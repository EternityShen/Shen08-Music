//! `audio_lib` —— 基于 [Symphonia](https://github.com/pdeljanov/Symphonia)（解码）
//! 和 [CPAL](https://github.com/RustAudio/cpal)（输出）的桌面端音乐播放库。
//!
//! 支持 Windows / macOS / Linux。
//!
//! # 快速上手
//!
//! ```no_run
//! use audio_lib::{Player, SeekTo};
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), audio_lib::AudioError> {
//! // 先建一个空句柄，再加载音源（也可用 Player::open 一步到位）
//! let player = Player::new()?;
//! player.load_file("song.mp3")?;
//!
//! // 可选：注册播放结束回调
//! player.set_on_end(|| println!("播放结束"));
//!
//! player.play();
//! std::thread::sleep(Duration::from_secs(3));
//!
//! player.pause();
//! player.play();
//!
//! // 按时间跳转
//! player.seek_time(Duration::from_secs(30))?;
//! // 按百分比跳转
//! player.seek_percent(0.5)?;
//!
//! // 音量 / 声像
//! player.set_volume(0.8);
//! player.set_pan(0.0);
//!
//! println!("位置: {:?} / {:?}", player.position(), player.duration());
//! # Ok(())
//! # }
//! ```
//!
//! # 播放网络 URL
//!
//! 开启 `url` feature（默认开启）即可直接播放 HTTP(S) 链接：
//!
//! ```ignore
//! // 需要 `url` feature（默认开启）。这里用 ignore 跳过编译，
//! // 因为 doctest 无法按 feature 条件编译；真实链路由 tests/url_playback.rs 覆盖。
//! let player = audio_lib::Player::new()?;
//! player.play_url("https://example.com/song.mp3")?; // 加载并立即播放
//! ```
//!
//! 网络字节在后台线程用 HTTP Range 顺序拉取，Seek 会中断当前下载并从新位置重新拉取，
//! 因此即使在慢速网络下 Seek 也能及时响应。若只需本地文件，可用
//! `default-features = false` 关掉 `url` 以减小体积（不引入 TLS 依赖）。
//!
//! # 设计要点
//!
//! - **解码与播放解耦**：独立的解码线程把任意格式统一转成交错立体声 f32，
//!   经无锁 SPSC 环形缓冲喂给 CPAL 实时回调。解码抖动不影响音频连续性。
//! - **实时安全的回调**：CPAL 回调不加锁、不分配、不阻塞，缓冲饿死时补静音。
//!   均衡器系数用 `arc-swap` 原子换入，淡变/音量/声像是原子，捕获复用无锁环形缓冲。
//! - **格式支持**：Symphonia `all` feature，覆盖 mp3 / flac / wav / ogg-vorbis /
//!   aac-mp4 / opus / aiff / caf / mkv 等。
//! - **Seek**：支持按时长和按百分比两种方式，Seek 通过命令通道通知解码线程，
//!   清空缓冲后跳转，避免回调吃到陈旧数据。
//!
//! # 句柄用法
//!
//! `Player` 本身是 `Arc<Player>`，可以直接存起来当句柄用。
//! 均衡器、淡变、捕获也各自返回**自包含句柄**（共享底层状态），
//! 可以单独保存、跨线程传递，随时调用方法控制播放中的效果。
//!
//! ```no_run
//! use audio_lib::Player;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), audio_lib::AudioError> {
//! let player = Player::new()?;
//! player.load_file("song.mp3")?;
//!
//! // Player 本身就是句柄：克隆廉价，可存可传
//! let handle = player.clone();
//! std::thread::spawn(move || {
//!     handle.play();
//! });
//!
//! // 把均衡器 / 淡变句柄存下来，之后只通过句柄控制
//! let eq = player.eq();
//! let fade = player.fade();
//! eq.set_gains(&[3.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0]); // 预设
//! fade.set_fade_in(Duration::from_millis(500)); // 时长按 wall-clock 存，跨采样率有效
//! fade.set_fade_out(Duration::from_millis(500));
//!
//! // 捕获句柄同理：存起来，随时读最终输出做可视化
//! let capture = player.enable_capture();
//!
//! player.play();
//!
//! // 之后在别处（UI 线程等）通过保存的句柄调用即可
//! eq.set_band_gain(5, 4.0);          // 1kHz 段提 4dB
//! fade.start_fade_out();             // 手动淡出
//! let mut buf = [0f32; 1024];
//! let n = capture.pop(&mut buf);     // 读最终输出样本
//! # let _ = n;
//! # Ok(())
//! # }
//! ```
//!
//! # 附加功能
//!
//! - **均衡器**：10 段参数 EQ，[`Player::eq`] 返回句柄，`set_band_gain` /
//!   `set_gains` / `set_enabled` 即时生效。
//! - **淡入淡出**：[`Player::fade`] 返回句柄，设自动淡入/淡出时长（播放/暂停
//!   自动平滑），或 `start_fade_in` / `start_fade_out` 手动触发。
//! - **标签信息**：[`Player::metadata`] 取标题/艺术家/专辑/流派/年份等。
//! - **播放样本捕获**：[`Player::enable_capture`] 返回无锁环形缓冲句柄，
//!   读最终输出做可视化频谱、电平表或录音。

pub mod player;
pub mod ring_buffer;
pub mod eq;
pub mod fade;
pub mod metadata;
#[cfg(feature = "url")]
pub mod http_source;

pub use player::{AudioError, NotifyCallback, Player, Result, SeekTo};
pub use eq::{Biquad, Equalizer, EqState, EQ_BAND_FREQS};
pub use fade::{FadeMode, FadeState};
pub use metadata::TrackMetadata;
