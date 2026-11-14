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
//! let player = Player::open("song.mp3")?;
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
//! ```no_run
//! # fn main() -> Result<(), audio_lib::AudioError> {
//! let player = audio_lib::Player::open_url("https://example.com/song.mp3")?;
//! player.play();
//! # Ok(())
//! # }
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
//! # 附加功能
//!
//! - **均衡器**：10 段参数 EQ，[`Player::eq`] 返回句柄，`set_band_gain` /
//!   `set_gains` / `set_enabled` 即时生效。
//! - **淡入淡出**：[`Player::fade`] 设自动淡入/淡出时长，或手动触发一次淡变，
//!   播放/暂停时自动平滑，避免爆音。
//! - **标签信息**：[`Player::metadata`] 取标题/艺术家/专辑/流派/年份等。
//! - **播放样本捕获**：[`Player::enable_capture`] 拿到无锁环形缓冲，
//!   读最终输出做可视化频谱、电平表或录音。
//!
//! ```no_run
//! # fn main() -> Result<(), audio_lib::AudioError> {
//! let player = audio_lib::Player::open("song.mp3")?;
//!
//! // 10 段 EQ：低音提一点，高音提一点
//! player.eq().set_gains(&[3.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0]);
//!
//! // 淡入淡出各 0.5 秒，播放/暂停自动平滑
//! use std::time::Duration;
//! player.fade().set_fade_in(Duration::from_millis(500), player.sample_rate());
//! player.fade().set_fade_out(Duration::from_millis(500), player.sample_rate());
//!
//! // 读标签
//! if let Some(title) = &player.metadata().title {
//!     println!("正在播放：{title}");
//! }
//!
//! // 捕获输出做可视化
//! let capture = player.enable_capture();
//! player.play();
//! let mut buf = [0f32; 1024];
//! let n = capture.pop(&mut buf); // 拿到最终输出样本
//! # let _ = n;
//! # Ok(())
//! # }
//! ```

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
