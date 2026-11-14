//! 淡入淡出（Fade in / Fade out）。
//!
//! 在音频回调里给每个采样乘一个增益包络，避免切歌、暂停、结束时的爆音（咔哒声）。
//!
//! ## 实时安全
//!
//! 回调里**不加锁**：包络参数（模式、已走采样数、时长）是原子，控制线程写、
//! 回调读。每个采样只做几次乘加，开销可忽略。
//!
//! ## 触发方式
//!
//! - **自动**：[`Player::play`] 触发淡入，[`Player::pause`] 触发淡出。
//!   时长由 `set_fade_in` / `set_fade_out` 设定，默认 0（直通，无淡变）。
//! - **手动**：`fade_in_now()` / `fade_out_now()` 立即开始一次淡变，
//!   可用于自定义过渡逻辑。

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// 淡变模式。存成 u32 以便用原子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FadeMode {
    /// 无淡变，增益恒为 1。
    None = 0,
    /// 从 0 淡入到 1。
    In = 1,
    /// 从 1 淡出到 0。
    Out = 2,
}

impl FadeMode {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => FadeMode::In,
            2 => FadeMode::Out,
            _ => FadeMode::None,
        }
    }
}

/// 淡变包络状态。回调按帧推进 `elapsed`，算出当前增益乘上去。
#[derive(Debug, Default)]
pub struct FadeState {
    /// 当前模式（`FadeMode` as u32）。
    mode: AtomicU32,
    /// 本次淡变已经过多少**采样**（含历史，跨回调块累加）。
    elapsed: AtomicU64,
    /// 淡入时长（采样数）。
    fade_in_samples: AtomicU64,
    /// 淡出时长（采样数）。
    fade_out_samples: AtomicU64,
}

impl FadeState {
    /// 创建一个无淡变的状态。`sample_rate` 保留以备将来按采样率换算，当前未用。
    pub fn new(_sample_rate: u32) -> Self {
        Self {
            mode: AtomicU32::new(FadeMode::None as u32),
            elapsed: AtomicU64::new(0),
            fade_in_samples: AtomicU64::new(0),
            fade_out_samples: AtomicU64::new(0),
        }
    }

    // -- 配置（控制线程调用）--

    /// 设置自动淡入时长。0 表示不淡入。
    pub fn set_fade_in(&self, duration: Duration, sample_rate: u32) {
        let n = duration.as_secs_f64() * sample_rate as f64;
        self.fade_in_samples.store(n as u64, Ordering::Relaxed);
    }

    /// 设置自动淡出时长。0 表示不淡出。
    pub fn set_fade_out(&self, duration: Duration, sample_rate: u32) {
        let n = duration.as_secs_f64() * sample_rate as f64;
        self.fade_out_samples.store(n as u64, Ordering::Relaxed);
    }

    /// 当前配置的自动淡入时长（采样数）。
    pub fn fade_in_samples(&self) -> u64 {
        self.fade_in_samples.load(Ordering::Relaxed)
    }

    /// 当前配置的自动淡出时长（采样数）。
    pub fn fade_out_samples(&self) -> u64 {
        self.fade_out_samples.load(Ordering::Relaxed)
    }

    // -- 触发（控制线程调用）--

    /// 立即开始一次淡入（从当前音量 0 拉到 1）。
    pub fn start_fade_in(&self) {
        self.elapsed.store(0, Ordering::Relaxed);
        self.mode.store(FadeMode::In as u32, Ordering::Relaxed);
    }

    /// 立即开始一次淡出（从当前音量 1 降到 0）。
    pub fn start_fade_out(&self) {
        self.elapsed.store(0, Ordering::Relaxed);
        self.mode.store(FadeMode::Out as u32, Ordering::Relaxed);
    }

    /// 取消进行中的淡变，恢复直通。
    pub fn cancel(&self) {
        self.mode.store(FadeMode::None as u32, Ordering::Relaxed);
    }

    // -- 回调（音频线程调用）--

    /// 对 `interleaved` 里每帧计算增益并乘上。`n_channels` 为交错声道数。
    ///
    /// 淡变走完后自动切回 [`FadeMode::None`]（增益 1），不额外处理。
    #[inline]
    pub fn process_interleaved(&self, interleaved: &mut [f32], n_channels: usize) {
        let mode = FadeMode::from_u32(self.mode.load(Ordering::Relaxed));
        if mode == FadeMode::None {
            return;
        }
        let n_ch = n_channels.max(1);
        let frames = interleaved.len() / n_ch;
        let mut elapsed = self.elapsed.load(Ordering::Relaxed);

        // 本次淡变总时长（采样数）。
        let total = match mode {
            FadeMode::In => self.fade_in_samples.load(Ordering::Relaxed),
            FadeMode::Out => self.fade_out_samples.load(Ordering::Relaxed),
            FadeMode::None => unreachable!(),
        };

        if total == 0 {
            // 时长为 0 的淡变没有意义，直接结束。
            self.mode.store(FadeMode::None as u32, Ordering::Relaxed);
            return;
        }

        for frame in 0..frames {
            let g = match mode {
                FadeMode::In => {
                    let t = (elapsed as f64 / total as f64).min(1.0);
                    t as f32
                }
                FadeMode::Out => {
                    let t = (elapsed as f64 / total as f64).min(1.0);
                    (1.0 - t) as f32
                }
                FadeMode::None => unreachable!(),
            };
            for ch in 0..n_ch {
                let idx = frame * n_ch + ch;
                interleaved[idx] *= g;
            }
            elapsed += 1;
            if elapsed >= total {
                // 淡变走完：把剩余采样按终值（淡入=1，淡出=0）处理，然后结束。
                if mode == FadeMode::Out {
                    let start = (frame + 1) * n_ch;
                    interleaved[start..frames * n_ch].fill(0.0);
                }
                self.mode.store(FadeMode::None as u32, Ordering::Relaxed);
                self.elapsed.store(0, Ordering::Relaxed);
                return;
            }
        }
        self.elapsed.store(elapsed, Ordering::Relaxed);
    }

    /// 当前是否在淡变中。
    pub fn is_fading(&self) -> bool {
        FadeMode::from_u32(self.mode.load(Ordering::Relaxed)) != FadeMode::None
    }

    /// Seek / 换歌时重置，取消进行中的淡变。
    pub fn reset(&self) {
        self.mode.store(FadeMode::None as u32, Ordering::Relaxed);
        self.elapsed.store(0, Ordering::Relaxed);
    }
}
