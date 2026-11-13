//! `audio_lib` —— 基于 [Symphonia](https://github.com/pdeljanov/Symphonia)（解码）
//! 和 [CPAL](https://github.com/RustAudio/cpal)（输出）的跨平台音乐播放库。
//!
//! 同时支持桌面端（Windows / macOS / Linux）和 Android。
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
//! # 设计要点
//!
//! - **解码与播放解耦**：独立的解码线程把任意格式统一转成交错立体声 f32，
//!   经无锁 SPSC 环形缓冲喂给 CPAL 实时回调。解码抖动不影响音频连续性。
//! - **实时安全的回调**：CPAL 回调不加锁、不分配、不阻塞，缓冲饿死时补静音。
//! - **格式支持**：Symphonia `all` feature，覆盖 mp3 / flac / wav / ogg-vorbis /
//!   aac-mp4 / opus / aiff / caf / mkv 等。
//! - **Seek**：支持按时长和按百分比两种方式，Seek 通过命令通道通知解码线程，
//!   清空缓冲后跳转，避免回调吃到陈旧数据。

pub mod player;
pub mod ring_buffer;

pub use player::{AudioError, NotifyCallback, Player, Result, SeekTo};
