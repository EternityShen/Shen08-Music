//! 基于 Symphonia（解码）+ CPAL（输出）的音乐播放器。
//!
//! 架构：
//! - **解码线程**：用 Symphonia 打开文件、逐包解码，统一转成交错**立体声** f32
//!   后推入环形缓冲。收到 Seek 命令时清空缓冲并跳转。缓冲满时 park 在 Condvar 上。
//! - **环形缓冲**：无锁 SPSC（见 [`crate::ring_buffer`]），解耦解码节奏和实时回调。
//! - **CPAL 回调**：实时音频线程，从环形缓冲取采样，套音量/声像后写进输出。
//!   绝不加锁、不分配、不阻塞，缓冲空了就补静音。
//!
//! 这样解码抖动不会直接打到音频回调上，Seek 也只需要和环形缓冲 + 解码线程打交道。

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{PlayStreamError, SampleRate, Stream, StreamConfig};
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{
    FormatOptions, FormatReader, Packet, SeekMode, SeekTo as SymSeekTo,
};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::ring_buffer::RingBuffer;
#[cfg(feature = "url")]
use crate::http_source::HttpSource;

use crate::eq::{EqState, Equalizer, EQ_BAND_FREQS};
use crate::fade::FadeState;
use crate::metadata::TrackMetadata;

/// 解码线程在环形缓冲里攒够多少**帧**后开始退避（约 1.4 秒 @48kHz）。
const DEFAULT_BUFFER_FRAMES: usize = 1 << 16;
/// 缓冲满时退避/ park 的超时时长。也用作命令轮询间隔。
const PARK_TIMEOUT: Duration = Duration::from_millis(10);

// ---------------------------------------------------------------------------
// 错误类型
// ---------------------------------------------------------------------------

/// 播放器对外暴露的错误。
#[derive(Debug)]
pub enum AudioError {
    /// 打不开文件或读不出数据。
    Io(std::io::Error),
    /// 文件探测失败：不是识别的容器格式，或里面没有音频轨。
    Probe(String),
    /// Symphonia 底层解码/格式错误。
    Symphonia(SymphoniaError),
    /// 拿不到默认输出设备（无声卡 / Android 未正确初始化音频后端）。
    NoOutputDevice,
    /// 输出设备不支持所需的流配置。
    StreamConfig(String),
    /// 建立 CPAL 输出流失败。
    StreamBuild(cpal::BuildStreamError),
    /// 启动 CPAL 输出流失败。
    StreamPlay(PlayStreamError),
    /// 调用了需要先加载文件的方法，但尚未加载，或总时长未知（百分比 Seek）。
    NotReady,
    /// 内部线程创建失败。
    Thread(std::io::Error),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Probe(msg) => write!(f, "probe failed: {msg}"),
            Self::Symphonia(e) => write!(f, "decode error: {e}"),
            Self::NoOutputDevice => write!(f, "no default output device available"),
            Self::StreamConfig(msg) => write!(f, "unsupported stream config: {msg}"),
            Self::StreamBuild(e) => write!(f, "failed to build output stream: {e}"),
            Self::StreamPlay(e) => write!(f, "failed to start output stream: {e}"),
            Self::NotReady => write!(f, "player not ready for this operation"),
            Self::Thread(e) => write!(f, "failed to spawn thread: {e}"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) | Self::Thread(e) => Some(e),
            Self::Symphonia(e) => Some(e),
            Self::StreamBuild(e) => Some(e),
            Self::StreamPlay(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AudioError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<SymphoniaError> for AudioError {
    fn from(e: SymphoniaError) -> Self {
        Self::Symphonia(e)
    }
}

impl From<cpal::BuildStreamError> for AudioError {
    fn from(e: cpal::BuildStreamError) -> Self {
        Self::StreamBuild(e)
    }
}

impl From<PlayStreamError> for AudioError {
    fn from(e: PlayStreamError) -> Self {
        Self::StreamPlay(e)
    }
}

/// 便捷别名。
pub type Result<T> = std::result::Result<T, AudioError>;

// ---------------------------------------------------------------------------
// 回调 & Seek
// ---------------------------------------------------------------------------

/// 播放结束时的通知回调。
///
/// 在独立线程里触发（不是音频回调线程），可以安全地做 UI 跨线程投递等操作。
pub type NotifyCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// Seek 的目标位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeekTo {
    /// 跳转到指定时长。
    Time(Duration),
    /// 按百分比跳转（0.0 ~ 1.0，超出会 clamp）。需要已知总时长。
    Percent(f64),
}

impl SeekTo {
    /// 把目标换算成绝对时长。`total` 是总时长，未知时按 `None` 处理。
    fn resolve(self, total: Option<Duration>) -> Option<Duration> {
        match self {
            Self::Time(t) => Some(t),
            Self::Percent(p) => total.map(|t| t.mul_f64(p.clamp(0.0, 1.0))),
        }
    }
}

// ---------------------------------------------------------------------------
// 解码命令
// ---------------------------------------------------------------------------

/// 控制线程 -> 解码线程的命令。
#[derive(Debug, Clone, Copy)]
enum Cmd {
    /// 清空缓冲并跳到指定绝对时长。
    Seek(Duration),
    /// 回到文件头重新解码。
    Restart,
}

// ---------------------------------------------------------------------------
// 跨线程共享状态
// ---------------------------------------------------------------------------

/// 控制线程、解码线程、音频回调三方共享的状态。
struct Shared {
    // -- 音频参数（构造后不变，无锁读）--
    /// 解码采样率，用于时长/位置换算。
    sample_rate: u32,
    /// 输出声道数（当前实现恒为 2）。
    channels: usize,
    /// 音轨总帧数，未知为 0。
    total_frames: u64,

    // -- 播放控制（原子，无锁读写）--
    /// 音量 0.0 ~ 1.0，bit-cast 成 u32 存。
    volume: AtomicU32,
    /// 声像 -1.0（左）~ 1.0（右），量化成 i8：-64..=64，避免浮点原子。
    pan: AtomicI8,
    /// 是否暂停。true 时回调输出静音，且不再消费环形缓冲。
    paused: AtomicBool,
    /// 解码线程已读到文件尾。
    eof: AtomicBool,
    /// 正在 Seek：从发起 Seek 到解码线程落地新位置之间，回调输出静音且不消费缓冲，
    /// 避免把旧位置的采样算进 position / 播给用户。
    seeking: AtomicBool,
    /// 播放是否已结束（EOF 且缓冲耗尽后置位，用于一次性触发 on_end）。
    finished: AtomicBool,
    /// 控制线程请求解码线程退出。
    stop: AtomicBool,
    /// 当前绝对播放位置，单位纳秒。音频回调按帧推进，Seek 时由控制线程
    /// 直接写成目标位置（立即生效，UI 无需等解码落地）。
    position_ns: AtomicU64,

    // -- 环形缓冲 & 解码线程 park --
    /// 交错立体声环形缓冲。
    ring: RingBuffer,
    /// 解码线程在缓冲满时 park 在这里；控制线程 Seek / Drop 时 notify 唤醒。
    decode_pair: (Mutex<bool>, Condvar),
    /// 网络流中断句柄（仅 URL 播放用到）。Seek 时置位，打断阻塞在网络 read 上的解码线程，
    /// 让它赶紧回到循环处理 Seek 命令。本地文件为 None（读不会阻塞）。
    pub(crate) interrupt: Mutex<Option<Arc<AtomicBool>>>,

    // -- 均衡器 --
    /// 均衡器参数（系数用 ArcSwap 原子换入）。控制线程改，回调无锁读。
    eq: Equalizer,
    /// 均衡器各声道滤波器状态。只有音频回调读写，锁几乎无竞争。
    eq_state: Mutex<EqState>,

    // -- 淡入淡出 --
    /// 淡变包络状态（内部全原子，回调无锁读）。
    fade: FadeState,

    // -- 播放样本捕获 --
    /// 捕获环形缓冲（第二个 SPSC tap）。回调是生产者，`read_capture` 是消费者。
    capture_ring: Arc<RingBuffer>,
    /// 是否启用捕获。关闭时回调跳过 push，零开销。
    capture_enabled: AtomicBool,

    // -- 标签信息 --
    /// 打开文件时提取的标签，只读。
    metadata: TrackMetadata,

    // -- 结束回调 --
    /// 播放结束回调。用 Mutex<Option> 让音频回调能安全取走克隆。
    on_end: Mutex<Option<NotifyCallback>>,
}

impl Shared {
    fn volume_f32(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn pan_f32(&self) -> f32 {
        self.pan.load(Ordering::Relaxed) as f32 / 64.0
    }

    /// 已播放时长。Seek 时控制线程直接写成目标值，因此立即生效。
    fn position(&self) -> Duration {
        Duration::from_nanos(self.position_ns.load(Ordering::Relaxed))
    }

    /// 音轨总时长，未知为 None。
    fn duration(&self) -> Option<Duration> {
        if self.total_frames == 0 || self.sample_rate == 0 {
            None
        } else {
            Some(Duration::from_secs_f64(
                self.total_frames as f64 / self.sample_rate as f64,
            ))
        }
    }

    /// 唤醒可能 park 在缓冲满上的解码线程。
    fn wake_decoder(&self) {
        let Ok(mut guard) = self.decode_pair.0.lock() else {
            return;
        };
        *guard = true;
        self.decode_pair.1.notify_all();
    }
}

// ---------------------------------------------------------------------------
// 播放器
// ---------------------------------------------------------------------------

/// 一个可播放、暂停、Seek 的音频播放器。
///
/// 内部持有 CPAL 输出流和一个专用解码线程。构造后处于**暂停**态，
/// 需要显式调用 [`Player::play`] 才会发声。
///
/// 通常包在 `Arc` 里跨线程共享；`Player` 本身 `Send + Sync`。
pub struct Player {
    shared: Arc<Shared>,
    stream: Stream,
    decoder: Mutex<Option<JoinHandle<()>>>,
    cmd_tx: std::sync::mpsc::Sender<Cmd>,
}

impl fmt::Debug for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Player")
            .field("sample_rate", &self.sample_rate())
            .field("channels", &self.channels())
            .field("duration", &self.duration())
            .field("position", &self.position())
            .field("paused", &self.is_paused())
            .field("volume", &self.volume())
            .field("pan", &self.pan())
            .finish()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // 顺序：停流 -> 通知解码线程退出 -> 唤醒可能 park 的解码线程 -> join。
        let _ = self.stream.pause();
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake_decoder();
        if let Ok(mut guard) = self.decoder.lock()
            && let Some(handle) = guard.take()
        {
            let _ = handle.join();
        }
    }
}

impl Player {
    // -- 构建 ---------------------------------------------------------------

    /// 用默认输出设备打开一个音频文件并开始预解码。
    ///
    /// `Player` 建好输出流并启动解码线程，但处于**暂停**态，
    /// 需要显式调用 [`Player::play`] 才会发声。
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Arc<Self>> {
        let path = path.as_ref();
        let file = std::fs::File::open(path)?;
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_owned);
        Self::open_source(Box::new(file), ext.as_deref())
    }

    /// 从任意 `MediaSource`（字节流、内存 buffer 等）打开。
    ///
    /// `hint_ext` 用于辅助探测格式，通常是文件扩展名，如 `Some("mp3")`。
    pub fn open_source(
        source: Box<dyn MediaSource>,
        hint_ext: Option<&str>,
    ) -> Result<Arc<Self>> {
        Self::build(source, hint_ext, None)
    }

    /// 打开一个 HTTP(S) 音频 URL 并开始流式预解码（需要 `url` feature，默认开启）。
    ///
    /// 网络字节在后台线程用 Range 请求顺序拉取；Seek 会中断阻塞中的下载并从新位置重新拉取。
    /// 返回的 `Player` 处于**暂停**态，需要显式调用 [`Player::play`]。
    ///
    /// 注意：探测容器头需要先下载前几十 KB，首次 `open_url` 会阻塞到这部分到达。
    #[cfg(feature = "url")]
    pub fn open_url(url: impl AsRef<str>) -> Result<Arc<Self>> {
        let url = url.as_ref();
        // 从扩展名猜格式提示（去掉 query string）。
        let ext = url
            .split(['?', '#'])
            .next()
            .and_then(|p| p.rsplit('.').next())
            .filter(|e| e.len() <= 5 && !e.contains('/'));
        let source = HttpSource::new(url).map_err(AudioError::Io)?;
        let interrupt = source.interrupt_handle();
        Self::build(Box::new(source), ext, Some(interrupt))
    }

    fn build(
        source: Box<dyn MediaSource>,
        hint_ext: Option<&str>,
        interrupt: Option<Arc<AtomicBool>>,
    ) -> Result<Arc<Self>> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;

        // -- 探测格式 --
        let mut hint = Hint::new();
        if let Some(ext) = hint_ext {
            hint.with_extension(ext);
        }
        let mss = MediaSourceStream::new(source, Default::default());
        let probed = symphonia::default::get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        );
        let mut format = match probed {
            Ok(p) => p.format,
            Err(e) => return Err(AudioError::Probe(format!("{e}"))),
        };

        let track = format
            .tracks()
            .first()
            .ok_or_else(|| AudioError::Probe("no audio track found in the container".into()))?;
        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
        let total_frames = track.codec_params.n_frames.unwrap_or(0);
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;

        // -- 提取标签信息 --
        // 必须在把 format 移进解码线程之前读，否则拿不到 MetadataLog。
        let mut metadata = TrackMetadata::default();
        {
            let log = format.metadata();
            if let Some(rev) = log.current() {
                metadata = TrackMetadata::from_tags(rev.tags());
            }
        }

        // -- 选输出流配置：立体声 f32，优先解码采样率 --
        let supported = device
            .supported_output_configs()
            .map_err(|e| AudioError::StreamConfig(format!("{e}")))?
            .filter(|c| c.channels() == 2 && c.sample_format() == cpal::SampleFormat::F32)
            .collect::<Vec<_>>();
        let stream_config: StreamConfig = pick_stream_config(&supported, sample_rate)
            .ok_or_else(|| {
                AudioError::StreamConfig("no stereo f32 output config available".into())
            })?
            .into();

        // -- 共享状态 --
        // 捕获缓冲：约 1 秒的立体声样本，读不及时新样本会覆盖旧的。
        let capture_cap = (sample_rate as usize * 2).next_power_of_two();
        let shared = Arc::new(Shared {
            sample_rate,
            channels: 2,
            total_frames,
            volume: AtomicU32::new(1.0f32.to_bits()),
            pan: AtomicI8::new(0),
            paused: AtomicBool::new(true),
            eof: AtomicBool::new(false),
            seeking: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            position_ns: AtomicU64::new(0),
            ring: RingBuffer::with_capacity(DEFAULT_BUFFER_FRAMES * 2), // *2: 交错两声道
            decode_pair: (Mutex::new(false), Condvar::new()),
            interrupt: Mutex::new(interrupt),
            eq: Equalizer::new(sample_rate),
            eq_state: Mutex::new(EqState::new(2, EQ_BAND_FREQS.len())),
            fade: FadeState::new(sample_rate),
            capture_ring: Arc::new(RingBuffer::with_capacity(capture_cap)),
            capture_enabled: AtomicBool::new(false),
            metadata,
            on_end: Mutex::new(None),
        });

        // -- 命令通道 & 解码线程 --
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
        let dec_shared = shared.clone();
        let decoder_handle = thread::Builder::new()
            .name("audio-lib-decode".into())
            .spawn(move || run_decoder(dec_shared, cmd_rx, format, decoder, track_id))
            .map_err(|e| AudioError::Thread(std::io::Error::other(e.to_string())))?;

        // -- 建立输出流 --
        let cb_shared = shared.clone();
        let stream = device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| audio_callback(data, &cb_shared),
                move |_err| {
                    // 输出流设备级错误（如设备被拔出）。留扩展点，目前忽略。
                },
                None,
            )?;
        stream.play()?;

        Ok(Arc::new(Self {
            shared,
            stream,
            decoder: Mutex::new(Some(decoder_handle)),
            cmd_tx,
        }))
    }

    // -- 传输控制 -----------------------------------------------------------

    /// 开始播放 / 从暂停恢复。
    ///
    /// 如果已播放结束，会自动回到文件头重新开始。
    /// 若配置了自动淡入（[`FadeState::set_fade_in`](crate::fade::FadeState::set_fade_in)），
    /// 恢复时音量会从 0 平滑拉起，避免爆音。
    pub fn play(&self) {
        if self.shared.finished.load(Ordering::Relaxed) {
            self.restart();
        }
        // 配了淡入就先开始淡变，再解除暂停；回调会按包络拉起音量。
        if self.shared.fade.fade_in_samples() > 0 {
            self.shared.fade.start_fade_in();
        }
        self.shared.paused.store(false, Ordering::Relaxed);
        let _ = self.stream.play();
    }

    /// 暂停播放。
    ///
    /// 若配置了自动淡出（[`FadeState::set_fade_out`](crate::fade::FadeState::set_fade_out)），
    /// 会先让音量平滑降到 0 再真正暂停，避免切歌/暂停的咔哒声。
    /// 这个方法会阻塞到淡出完成（通常几十毫秒）；没配淡出则立即暂停。
    pub fn pause(&self) {
        let fade_out = self.shared.fade.fade_out_samples();
        if fade_out > 0 && !self.is_paused() {
            let sr = self.sample_rate().max(1);
            self.shared.fade.start_fade_out();
            // 等淡变走完再暂停。回调按采样率推进 elapsed，这里按 wall-clock 等。
            let wait = Duration::from_secs_f64(fade_out as f64 / sr as f64);
            std::thread::sleep(wait + Duration::from_millis(5));
        }
        self.shared.paused.store(true, Ordering::Relaxed);
        self.shared.fade.cancel();
        let _ = self.stream.pause();
    }

    /// 暂停/播放切换，返回切换后是否在播放。
    pub fn toggle_play_pause(&self) -> bool {
        if self.is_paused() {
            self.play();
            true
        } else {
            self.pause();
            false
        }
    }

    /// 是否处于暂停状态。
    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    /// 是否已播放结束（EOF 且缓冲耗尽）。
    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }

    /// 回到文件头重新开始（会清空缓冲并让解码线程从头解码）。
    pub fn restart(&self) {
        self.shared.finished.store(false, Ordering::Relaxed);
        // 置 seeking 让回调立刻静音，直到解码线程回到文件头。
        self.shared.seeking.store(true, Ordering::Relaxed);
        self.shared.position_ns.store(0, Ordering::Relaxed);
        self.shared.ring.clear();
        self.shared.wake_decoder();
        let _ = self.cmd_tx.send(Cmd::Restart);
    }

    // -- 均衡器 / 淡变 / 标签 / 捕获 ----------------------------------------

    /// 均衡器句柄。改增益 / 启停后回调下一个 block 生效。
    pub fn eq(&self) -> &Equalizer {
        &self.shared.eq
    }

    /// 淡入淡出句柄。可设自动淡入/淡出时长，或手动触发一次淡变。
    pub fn fade(&self) -> &FadeState {
        &self.shared.fade
    }

    /// 打开文件时提取的标签信息。容器没写标签时字段为 `None`。
    pub fn metadata(&self) -> &TrackMetadata {
        &self.shared.metadata
    }

    /// 启用播放样本捕获，返回一个可读的环形缓冲句柄。
    ///
    /// 回调会把**最终输出**（过了 EQ、淡变、音量、声像之后的样本）写进这个缓冲，
    /// 适合做可视化频谱、电平表、录音。缓冲固定容量，读不及时新样本会覆盖旧的。
    ///
    /// 句柄和 `Player` 独立，`Player` drop 后句柄里残留的数据仍可读。
    pub fn enable_capture(&self) -> Arc<RingBuffer> {
        self.shared.capture_ring.clear();
        self.shared.capture_enabled.store(true, Ordering::Relaxed);
        self.shared.capture_ring.clone()
    }

    /// 关闭捕获。回调不再写入，句柄里已捕获的数据仍可继续读。
    pub fn disable_capture(&self) {
        self.shared.capture_enabled.store(false, Ordering::Relaxed);
    }

    /// 是否启用捕获。
    pub fn is_capture_enabled(&self) -> bool {
        self.shared.capture_enabled.load(Ordering::Relaxed)
    }

    // -- Seek ---------------------------------------------------------------

    /// 跳转到目标位置。
    pub fn seek(&self, target: SeekTo) -> Result<()> {
        let Some(abs) = target.resolve(self.duration()) else {
            return Err(AudioError::NotReady);
        };
        self.seek_abs(abs);
        Ok(())
    }

    /// 跳到指定时长（便捷方法）。
    pub fn seek_time(&self, t: Duration) -> Result<()> {
        self.seek(SeekTo::Time(t))
    }

    /// 按百分比跳转（0.0 ~ 1.0）。
    pub fn seek_percent(&self, p: f64) -> Result<()> {
        self.seek(SeekTo::Percent(p))
    }

    fn seek_abs(&self, abs: Duration) {
        self.shared.finished.store(false, Ordering::Relaxed);
        // 立即把位置写成目标值，UI 无需等解码落地。
        self.shared
            .position_ns
            .store(abs.as_nanos() as u64, Ordering::Relaxed);
        // 置 seeking 让回调立刻静音并停止推进 position，
        // 直到解码线程落地到新位置（do_seek 里清除）。
        self.shared.seeking.store(true, Ordering::Relaxed);
        // URL 播放：先置中断，打断可能阻塞在网络 read 上的解码线程，
        // 让它能回到循环处理下面的 Seek 命令。必须在发命令前置位。
        if let Ok(guard) = self.shared.interrupt.lock()
            && let Some(handle) = guard.as_ref()
        {
            handle.store(true, Ordering::Relaxed);
        }
        self.shared.wake_decoder();
        let _ = self.cmd_tx.send(Cmd::Seek(abs));
    }

    // -- 状态查询 -----------------------------------------------------------

    /// 当前播放位置。
    pub fn position(&self) -> Duration {
        self.shared.position()
    }

    /// 音轨总时长，未知（容器没写）返回 None。
    pub fn duration(&self) -> Option<Duration> {
        self.shared.duration()
    }

    /// 解码采样率。
    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate
    }

    /// 输出声道数。
    pub fn channels(&self) -> usize {
        self.shared.channels
    }

    /// 当前环形缓冲里积压的帧数（诊断用）。
    pub fn buffered_frames(&self) -> usize {
        self.shared.ring.len() / self.shared.channels
    }

    // -- 音量 / 声像 --------------------------------------------------------

    /// 设置音量，0.0（静音）~ 1.0（原声）。超出范围会被 clamp。
    pub fn set_volume(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.shared.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    /// 当前音量。
    pub fn volume(&self) -> f32 {
        self.shared.volume_f32()
    }

    /// 设置声像，-1.0（全左）~ 1.0（全右），0.0 居中。超出范围会被 clamp。
    pub fn set_pan(&self, p: f32) {
        let p = (p.clamp(-1.0, 1.0) * 64.0).round() as i8;
        self.shared.pan.store(p, Ordering::Relaxed);
    }

    /// 当前声像。
    pub fn pan(&self) -> f32 {
        self.shared.pan_f32()
    }

    // -- 回调 ---------------------------------------------------------------

    /// 注册播放结束回调（EOF 且缓冲耗尽时触发一次）。
    pub fn set_on_end<F: Fn() + Send + Sync + 'static>(&self, f: F) {
        *self.shared.on_end.lock().unwrap() = Some(Arc::new(f));
    }

    /// 清除播放结束回调。
    pub fn clear_on_end(&self) {
        *self.shared.on_end.lock().unwrap() = None;
    }
}

// ---------------------------------------------------------------------------
// 解码线程
// ---------------------------------------------------------------------------

/// 解码线程主循环。
fn run_decoder(
    shared: Arc<Shared>,
    cmd_rx: std::sync::mpsc::Receiver<Cmd>,
    mut format: Box<dyn FormatReader>,
    mut decoder: Box<dyn Decoder>,
    track_id: u32,
) {
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut scratch: Vec<f32> = Vec::new();
    let mut decode_finished = false;

    loop {        if shared.stop.load(Ordering::Relaxed) {
            return;
        }

        // 非阻塞处理命令
        if let Ok(cmd) = cmd_rx.try_recv()
            && apply_cmd(&shared, &mut format, track_id, cmd)
        {
            decode_finished = false;
        }

        if decode_finished {
            park_decoder(&shared);
            continue;
        }

        // 取下一个包
        let packet: Packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                shared.eof.store(true, Ordering::Relaxed);
                decode_finished = true;
                continue;
            }
            Err(_) => {
                // URL Seek 会中断阻塞中的网络 read（ErrorKind::Interrupted），
                // 这不是 EOF。清掉中断标记，短暂让出，回到循环顶部处理待处理的 Seek 命令。
                let interrupted = shared
                    .interrupt
                    .lock()
                    .ok()
                    .and_then(|g| g.clone())
                    .is_some_and(|h| h.load(Ordering::Relaxed));
                if interrupted {
                    if let Ok(g) = shared.interrupt.lock()
                        && let Some(h) = g.as_ref()
                    {
                        h.store(false, Ordering::Relaxed);
                    }
                    std::thread::sleep(PARK_TIMEOUT);
                    continue;
                }
                // ResetRequired / 其他不可恢复错误：当作到尾处理。
                shared.eof.store(true, Ordering::Relaxed);
                decode_finished = true;
                continue;
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        // 解码
        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue, // 单包损坏，跳过
            Err(_) => {
                shared.eof.store(true, Ordering::Relaxed);
                decode_finished = true;
                continue;
            }
        };

        // 转成交错立体声 f32
        fill_stereo(decoded, &mut sample_buf, &mut scratch);
        if scratch.is_empty() {
            continue;
        }

        // 推入环形缓冲，满了 park（可被 Seek/Drop 唤醒）
        let mut offset = 0;
        while offset < scratch.len() {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            let written = shared.ring.push(&scratch[offset..]);
            offset += written;
            if written == 0
                && park_decoder(&shared)
            {
                // 被唤醒且可能收到了命令，回到外层处理
                break;
            }
        }
    }
}

/// 应用一条命令，返回是否需要重置 `decode_finished`。
fn apply_cmd(
    shared: &Arc<Shared>,
    format: &mut Box<dyn FormatReader>,
    track_id: u32,
    cmd: Cmd,
) -> bool {
    match cmd {
        Cmd::Seek(target) => {
            do_seek(shared, format, track_id, target);
            true
        }
        Cmd::Restart => {
            do_seek(shared, format, track_id, Duration::ZERO);
            true
        }
    }
}

/// 执行 Seek：清缓冲、seek 格式读取器，落地后解除 seeking。
/// 位置已由控制线程在发起 Seek 时写成目标值，这里不再改写。
fn do_seek(shared: &Shared, format: &mut Box<dyn FormatReader>, track_id: u32, target: Duration) {
    shared.ring.clear();
    shared.eof.store(false, Ordering::Relaxed);
    shared.finished.store(false, Ordering::Relaxed);

    let _ = format.seek(
        SeekMode::Accurate,
        SymSeekTo::Time {
            time: Time::from(target),
            track_id: Some(track_id),
        },
    );

    // 落地到新位置，解除 seeking，回调恢复消费并从目标位置继续推进 position。
    shared.seeking.store(false, Ordering::Relaxed);

    // 重置 EQ 滤波器历史，避免上一个位置的残留采样在跳转后造成瞬态爆音。
    // 这段只在解码线程跑，回调可能并发 try_lock；拿不到锁就跳过（回调侧自己会处理）。
    if let Ok(mut eq_state) = shared.eq_state.lock() {
        eq_state.reset();
    }
    // 取消进行中的淡变，避免跳转后音量卡在中间值。
    shared.fade.reset();
}

/// park 解码线程一段时间，或直到被唤醒（Seek/Drop）。返回是否被提前唤醒。
fn park_decoder(shared: &Shared) -> bool {
    if shared.stop.load(Ordering::Relaxed) {
        return false;
    }
    let (mutex, cv) = &shared.decode_pair;
    let Ok(mut woke) = mutex.lock() else {
        return false;
    };
    // 用超时 park：既能在被唤醒时立刻响应，又能在回调消费数据后自动重试。
    let (guard, _timeout) = cv
        .wait_timeout(woke, PARK_TIMEOUT)
        .unwrap_or_else(|e| e.into_inner());
    woke = guard;
    let was_woken = *woke;
    *woke = false;
    was_woken
}

/// 把 Symphonia 解码输出统一转成交错**立体声** f32，写进 `scratch`。
///
/// 单声道复制到双声道；>2 声道取前两声道。`sample_buf` 复用堆缓冲避免分配，
/// 遇到更大的包时按需扩容。
fn fill_stereo(
    decoded: AudioBufferRef<'_>,
    sample_buf: &mut Option<SampleBuffer<f32>>,
    scratch: &mut Vec<f32>,
) {
    let spec = *decoded.spec();
    let channels = spec.channels.count().max(1);
    let frames = decoded.frames() as u64;

    // SampleBuffer 构造后容量固定，遇到更大的包需要重建。
    let needed = frames * channels as u64;
    let reusable = sample_buf
        .as_ref()
        .is_some_and(|b| b.capacity() as u64 >= needed);
    if !reusable {
        *sample_buf = Some(SampleBuffer::<f32>::new(frames, spec));
    }
    let buf = sample_buf.as_mut().unwrap();
    buf.copy_interleaved_ref(decoded);
    let src = buf.samples();

    scratch.clear();
    match channels {
        1 => {
            scratch.reserve(src.len() * 2);
            for s in src {
                scratch.push(*s);
                scratch.push(*s);
            }
        }
        2 => {
            scratch.extend_from_slice(src);
        }
        _ => {
            // 多声道：简单下混，取前两声道。
            scratch.reserve(frames as usize * 2);
            for f in 0..frames as usize {
                let base = f * channels;
                let l = src[base];
                let r = if channels > 1 { src[base + 1] } else { l };
                scratch.push(l);
                scratch.push(r);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 音频回调
// ---------------------------------------------------------------------------

/// CPAL 实时音频回调：从环形缓冲取数据、套音量/声像、写输出。
///
/// 绝不加锁、不分配、不阻塞。缓冲数据不足时补静音。
fn audio_callback(out: &mut [f32], shared: &Shared) {
    // 暂停、已结束、或正在 Seek：静音，且不消费缓冲。
    // seeking 期间保持 position 为 0，直到解码线程落地新位置。
    if shared.paused.load(Ordering::Relaxed)
        || shared.finished.load(Ordering::Relaxed)
        || shared.seeking.load(Ordering::Relaxed)
    {
        out.fill(0.0);
        return;
    }

    let channels = shared.channels.max(1);
    let frames = out.len() / channels;
    let want = frames * channels;

    // 栈上临时缓冲；CPAL 回调通常每块几百~几千采样，4096 足够。
    // 超出时按 4096 分批（罕见）。
    let mut tmp = [0f32; 4096];
    let chunk = want.min(tmp.len());
    let got = shared.ring.pop(&mut tmp[..chunk]);

    if got == 0 {
        out.fill(0.0);
        if shared.eof.load(Ordering::Relaxed) {
            maybe_finish(shared);
        }
        return;
    }

    // -- 均衡器：先过滤波器，再叠音量/声像/淡变 --
    // eq_state 只由音频回调读写，锁几乎无竞争；持锁范围仅这一小块。
    // try_lock 失败说明有别的线程短暂持有（极罕见），这块跳过 EQ，
    // 只影响几毫秒的滤波，不影响稳定性。
    if shared.eq.is_enabled()
        && let Ok(mut eq_state) = shared.eq_state.try_lock()
    {
        eq_state.process_interleaved(&shared.eq, &mut tmp[..got]);
    }

    // -- 淡入淡出：按包络给每个采样乘增益 --
    shared.fade.process_interleaved(&mut tmp[..got], channels);

    let volume = shared.volume_f32();
    let pan = shared.pan_f32();
    // 等功率声像：pan=-1 全左，pan=1 全右。
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    let lg = angle.cos() * volume;
    let rg = angle.sin() * volume;

    let frames_got = got / channels;
    for f in 0..frames_got {
        let l = tmp[f * channels];
        let r = tmp[f * channels + 1];
        out[f * channels] = l * lg;
        out[f * channels + 1] = r * rg;
    }
    // 请求帧多于得到帧时，尾部补静音。
    for s in &mut out[frames_got * channels..] {
        *s = 0.0;
    }

    // -- 播放样本捕获：把最终输出（EQ/淡变/音量/声像之后）写进捕获缓冲 --
    // 满了 push 会丢新样本（写不进去），适合电平表/频谱这类可容忍丢失的场景。
    if shared.capture_enabled.load(Ordering::Relaxed) {
        let out_samples = &out[..frames_got * channels];
        let _ = shared.capture_ring.push(out_samples);
    }

    // 按实际消费的帧数推进绝对位置（纳秒）。
    if shared.sample_rate > 0 {
        let advance_ns = (frames_got as u128 * 1_000_000_000u128) / shared.sample_rate as u128;
        shared
            .position_ns
            .fetch_add(advance_ns as u64, Ordering::Relaxed);
    }

    // 播完检测：EOF 且缓冲已空且这块没填满
    if shared.eof.load(Ordering::Relaxed)
        && shared.ring.is_empty()
        && frames_got < frames
    {
        maybe_finish(shared);
    }
}

/// 标记播放结束并触发 on_end 回调（只触发一次）。
fn maybe_finish(shared: &Shared) {
    if !shared
        .finished
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
        .is_ok()
    {
        return; // 已经触发过了
    }
    let cb = shared.on_end.lock().ok().and_then(|g| g.clone());
    if let Some(cb) = cb {
        // 独立线程触发，避免阻塞音频回调。
        thread::Builder::new()
            .name("audio-lib-on-end".into())
            .spawn(move || cb())
            .ok();
    }
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

/// 从设备支持的配置范围里挑一个最接近 `prefer_rate` 的立体声 f32 配置。
fn pick_stream_config(
    supported: &[cpal::SupportedStreamConfigRange],
    prefer_rate: u32,
) -> Option<cpal::SupportedStreamConfig> {
    // 优先在覆盖 prefer_rate 的范围里精确匹配采样率
    let exact = supported
        .iter()
        .find(|c| {
            c.min_sample_rate().0 <= prefer_rate && c.max_sample_rate().0 >= prefer_rate
        })
        .and_then(|c| c.try_with_sample_rate(SampleRate(prefer_rate)));
    if let Some(cfg) = exact {
        return Some(cfg);
    }
    // 否则用第一个范围的上限采样率
    supported.first().map(|c| (*c).with_max_sample_rate())
}
