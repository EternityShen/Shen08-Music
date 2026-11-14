//! 参数均衡器（Parametric EQ）。
//!
//! 10 段固定中心频率，每段一个 RBJ peaking biquad 滤波器。解码后的采样在
//! 音频回调里按 段 -> 采样 逐级过滤。
//!
//! ## 实时安全
//!
//! 回调里**不加锁、不分配**：滤波器系数打包成 `Arc<[Biquad]>`，控制线程用
//! [`arc_swap::ArcSwap`] 原子地整体换入，回调里一次无锁 `load()` 拿到快照。
//! 各声道的滤波器历史状态由 [`EqState`] 持有（只被音频回调读写）。
//!
//! ## 设计要点
//!
//! 系数（[`Biquad`]，不可变，放 `ArcSwap`）和历史状态（[`BiquadState`]，可变，
//! 放 `EqState`）**严格分离**。处理时把系数借给状态：
//! `biquad.process(&mut state, x)`。这样换系数不会丢历史，也不会把系数误存成
//! 直通默认值。
//!
//! ## 滤波器数学
//!
//! RBJ peaking EQ（`Audio EQ Cookbook`）：
//! ```text
//! A     = 10^(gain_db / 40)
//! w0    = 2π·f0 / fs
//! α     = sin(w0) / (2·Q)
//! b0 = 1 + α·A    b1 = -2·cos(w0)    b2 = 1 - α·A
//! a0 = 1 + α/A    a1 = -2·cos(w0)    a2 = 1 - α/A
//! ```
//! 所有系数最后除以 `a0` 归一化（`a0 = 1`），回调里少一次除法。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

/// 默认 10 段中心频率（Hz）：次低音 -> 空气感。
pub const EQ_BAND_FREQS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

/// 默认每段 Q 值。中等带宽，段间 overlap 适中，听感自然。
pub const DEFAULT_EQ_Q: f32 = 1.0;

/// 单个 biquad 滤波器的归一化系数（`a0 = 1`）。
///
/// 纯系数，不含历史状态（历史在 [`BiquadState`] 里），因此可安全地放进
/// `ArcSwap` 在控制线程和音频回调间共享。
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        // 默认直通：y = x。
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

impl Biquad {
    /// 构造一个 RBJ peaking EQ 滤波器。
    ///
    /// - `freq`：中心频率（Hz）。
    /// - `gain_db`：增益（dB），正提升负衰减，0 为直通。
    /// - `sample_rate`：采样率（Hz）。
    /// - `q`：品质因数，越大带宽越窄。
    pub fn peaking(freq: f32, gain_db: f32, sample_rate: f32, q: f32) -> Self {
        // 增益为 0 时是恒等滤波器，返回直通，省去三角函数。
        if gain_db.abs() < 1e-4 {
            return Self::default();
        }

        let a = 10f32.powf(gain_db / 40.0);
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        // w0 接近 π（freq 接近 Nyquist）时数值发散，clamp 一下保证稳定。
        let w0 = w0.min(std::f32::consts::PI - 1e-3);
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos_w0) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    /// 处理单个采样，历史状态存于 `st`。Direct Form I：
    /// `y[n] = b0·x[n] + b1·x[n-1] + b2·x[n-2] - a1·y[n-1] - a2·y[n-2]`。
    #[inline(always)]
    pub fn process(&self, st: &mut BiquadState, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * st.x1 + self.b2 * st.x2
            - self.a1 * st.y1
            - self.a2 * st.y2;
        st.x2 = st.x1;
        st.x1 = x;
        st.y2 = st.y1;
        st.y1 = y;
        y
    }
}

/// 单个 biquad 滤波器的历史状态（直接形式 I）。
///
/// 只被音频回调读写，随声道 / 段各持一份。
#[derive(Clone, Copy, Debug, Default)]
pub struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    /// 重置历史状态（Seek / 换歌时避免残留采样造成爆音）。
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// 一组滤波器系数（每段一个）。回调里按声道各持有独立状态。
type Coeffs = Arc<[Biquad]>;

/// 均衡器的可调参数（增益 + 采样率 + Q）。控制线程读写，换采样率或改增益时重算系数。
#[derive(Clone)]
struct EqParams {
    /// 当前采样率。换源时可能变化，需据此重算全部系数。
    sample_rate: u32,
    /// 每段 Q 值。
    q: f32,
    /// 每段增益（dB）。保存下来，采样率变化时据此重建系数。
    gains: Vec<f32>,
}

/// 参数均衡器。
///
/// 这是个**自包含句柄**：克隆共享同一份系数、启停状态和参数（都走 `Arc`），
/// 保存一份随时调用方法，改动立即作用到正在播放的均衡器。
/// 每个声道需要独立滤波器状态，由回调侧持有（见 [`EqState`]）。
#[derive(Clone)]
pub struct Equalizer {
    /// 当前系数快照。控制线程 `store`，回调 `load`，无锁。
    coeffs: Arc<ArcSwap<Coeffs>>,
    /// 是否启用。关闭时回调直接跳过滤波（省 CPU）。Arc 共享，克隆句柄控制同一状态。
    enabled: Arc<AtomicBool>,
    /// 可调参数（增益/采样率/Q）。`Mutex` 保护，只在控制线程改；换采样率时重算系数。
    params: Arc<Mutex<EqParams>>,
    /// 段数。
    bands: usize,
}

impl Equalizer {
    /// 创建一个 10 段均衡器，所有段增益为 0（直通）。
    pub fn new(sample_rate: u32) -> Self {
        Self::with_gains(sample_rate, &[0.0; 10])
    }

    /// 用给定的各段增益（dB）创建均衡器。`gains_db` 不足 10 段的部分补 0。
    pub fn with_gains(sample_rate: u32, gains_db: &[f32]) -> Self {
        let bands = EQ_BAND_FREQS.len();
        let gains: Vec<f32> = (0..bands)
            .map(|i| gains_db.get(i).copied().unwrap_or(0.0))
            .collect();
        let coeffs = Self::build_coeffs(&EQ_BAND_FREQS, &gains, sample_rate, DEFAULT_EQ_Q);
        Self {
            coeffs: Arc::new(ArcSwap::from_pointee(coeffs)),
            enabled: Arc::new(AtomicBool::new(true)),
            params: Arc::new(Mutex::new(EqParams {
                sample_rate,
                q: DEFAULT_EQ_Q,
                gains,
            })),
            bands,
        }
    }

    /// 根据频率表 + 增益 + 采样率 + Q 构建一整组系数。
    fn build_coeffs(freqs: &[f32], gains: &[f32], sample_rate: u32, q: f32) -> Coeffs {
        let coeffs: Vec<Biquad> = freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| {
                let g = gains.get(i).copied().unwrap_or(0.0);
                Biquad::peaking(f, g, sample_rate as f32, q)
            })
            .collect();
        Arc::from(coeffs.into_boxed_slice())
    }

    /// 设置某一段的增益（dB）。`band` 越界则忽略。
    ///
    /// 更新保存的增益并重算该段系数，原子换入（微秒级），回调侧下一个 block 生效。
    /// 段间是级联的，改一段只影响该段的响应，其余段系数保持不变。
    pub fn set_band_gain(&self, band: usize, gain_db: f32) {
        if band >= self.bands {
            return;
        }
        // 更新参数里保存的增益，再用当前采样率重算该段系数。
        let (gains, sample_rate, q) = {
            let Ok(mut p) = self.params.lock() else {
                return;
            };
            p.gains[band] = gain_db;
            (p.gains.clone(), p.sample_rate, p.q)
        };
        let old = self.coeffs.load();
        let mut new: Vec<Biquad> = old.iter().copied().collect();
        new[band] = Biquad::peaking(EQ_BAND_FREQS[band], gains[band], sample_rate as f32, q);
        self.coeffs.store(Arc::new(Arc::from(new.into_boxed_slice())));
    }

    /// 一次性设置所有段增益（dB）。长度不足补 0，超出忽略。
    pub fn set_gains(&self, gains_db: &[f32]) {
        let (gains, sample_rate, q) = {
            let Ok(mut p) = self.params.lock() else {
                return;
            };
            for (i, g) in gains_db.iter().enumerate().take(self.bands) {
                p.gains[i] = *g;
            }
            (p.gains.clone(), p.sample_rate, p.q)
        };
        let coeffs = Self::build_coeffs(&EQ_BAND_FREQS, &gains, sample_rate, q);
        self.coeffs.store(Arc::new(coeffs));
    }

    /// 采样率变化时调用（换源）。保留当前各段增益，按新采样率重算全部系数。
    pub fn set_sample_rate(&self, sample_rate: u32) {
        let (gains, q) = {
            let Ok(mut p) = self.params.lock() else {
                return;
            };
            p.sample_rate = sample_rate;
            (p.gains.clone(), p.q)
        };
        let coeffs = Self::build_coeffs(&EQ_BAND_FREQS, &gains, sample_rate, q);
        self.coeffs.store(Arc::new(coeffs));
    }

    /// 当前采样率（重算系数用）。
    pub fn sample_rate(&self) -> u32 {
        self.params.lock().map(|p| p.sample_rate).unwrap_or(0)
    }

    /// 启用/禁用均衡器。禁用时回调跳过滤波。
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// 是否启用。
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 段数。
    pub fn bands(&self) -> usize {
        self.bands
    }

    /// 第 `band` 段的中心频率（Hz）。
    pub fn band_freq(&self, band: usize) -> Option<f32> {
        EQ_BAND_FREQS.get(band).copied()
    }

    /// 第 `band` 段当前配置的增益（dB）。`band` 越界返回 `None`。
    ///
    /// 这是保存的增益值，换源（采样率变化）后依然保留。
    pub fn band_gain(&self, band: usize) -> Option<f32> {
        self.params.lock().ok()?.gains.get(band).copied()
    }

    /// 当前系数快照（音频回调用）。也可用于检查实际生效的滤波器系数。
    pub fn coeffs(&self) -> Arc<[Biquad]> {
        // coeffs 是 Arc<ArcSwap<Arc<[Biquad]>>>，load_full 给出 Arc<Arc<[Biquad]>>，
        // 再 clone 内层得到 Arc<[Biquad]>。
        Arc::clone(self.coeffs.load_full().as_ref())
    }
}

/// 每声道每段的滤波器状态。回调侧持有，随声道数初始化。
pub struct EqState {
    /// `[channel][band]` 的滤波器历史状态。
    channels: Vec<Vec<BiquadState>>,
}

impl EqState {
    /// 为 `n_channels` 个声道创建处理状态。
    pub fn new(n_channels: usize, bands: usize) -> Self {
        Self {
            channels: vec![vec![BiquadState::default(); bands]; n_channels],
        }
    }

    /// 对交错采样应用均衡，就地修改。`n_channels` 为交错声道数。
    #[inline]
    pub fn process_interleaved(&mut self, eq: &Equalizer, interleaved: &mut [f32]) {
        if !eq.is_enabled() {
            return;
        }
        let coeffs = eq.coeffs();
        let bands = coeffs.len();
        let n_ch = self.channels.len();
        if bands == 0 || n_ch == 0 {
            return;
        }
        // 交错布局：每 frame 有 n_ch 个采样，第 ch 个属于声道 ch。
        // 逐声道把采样依次过所有段（段间级联）。
        for chunk in interleaved.chunks_mut(n_ch) {
            for ch in 0..n_ch.min(chunk.len()) {
                let mut s = chunk[ch];
                let state = &mut self.channels[ch];
                for band in 0..bands {
                    s = coeffs[band].process(&mut state[band], s);
                }
                chunk[ch] = s;
            }
        }
    }

    /// 重置所有声道所有段的历史状态。
    pub fn reset(&mut self) {
        for ch in &mut self.channels {
            for b in ch {
                b.reset();
            }
        }
    }
}
