//! 集成测试：用一个程序化生成的 WAV 文件跑通完整的 解码 -> 环形缓冲 -> 回调 链路。
//!
//! WAV 用 `hound` 生成太重，这里手写一个最小的 PCM WAV 头 + 数据，避免额外依赖。

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use audio_lib::Player;
use cpal::traits::HostTrait;

/// 生成一个单声道 16-bit PCM WAV，返回 (路径, 总采样数)。
fn make_wav(path: &PathBuf, sample_rate: u32, seconds: f64) -> u32 {
    let n_samples = (sample_rate as f64 * seconds) as u32;
    let data_len = n_samples * 2; // 16-bit = 2 bytes
    let file_len = 36 + data_len;

    let mut f = std::fs::File::create(path).unwrap();
    // RIFF header
    f.write_all(b"RIFF").unwrap();
    f.write_all(&file_len.to_le_bytes()).unwrap();
    f.write_all(b"WAVE").unwrap();
    // fmt chunk
    f.write_all(b"fmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap(); // chunk size
    f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&(sample_rate * 2).to_le_bytes()).unwrap(); // byte rate
    f.write_all(&2u16.to_le_bytes()).unwrap(); // block align
    f.write_all(&16u16.to_le_bytes()).unwrap(); // bits per sample
    // data chunk
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();

    // 440Hz 正弦波
    for i in 0..n_samples {
        let t = i as f64 / sample_rate as f64;
        let s = (2.0 * std::f64::consts::PI * 440.0 * t).sin();
        let v = (s * 0.5 * i16::MAX as f64) as i16;
        f.write_all(&v.to_le_bytes()).unwrap();
    }
    n_samples
}

/// 只有在有默认输出设备时才跑完整链路（CI 无声卡环境会跳过）。
fn has_output_device() -> bool {
    cpal_host_has_device()
}

fn cpal_host_has_device() -> bool {
    // 复用 cpal 间接判断；audio_lib 不导出 host，这里用 cpal 直接查。
    cpal::default_host().default_output_device().is_some()
}

#[test]
fn full_pipeline_play_pause_seek() {
    if !has_output_device() {
        eprintln!("跳过：无默认输出设备");
        return;
    }

    let dir = std::env::temp_dir();
    let path = dir.join("audio_lib_test_tone.wav");
    let total = make_wav(&path, 44_100, 5.0);
    let total_dur = Duration::from_secs_f64(total as f64 / 44_100.0);

    let player = Player::open(&path).expect("open wav");
    assert_eq!(player.sample_rate(), 44_100);
    assert_eq!(player.channels(), 2);
    assert_eq!(player.duration(), Some(total_dur));
    assert!(player.is_paused(), "构造后应处于暂停态");

    // 播放
    player.play();
    assert!(!player.is_paused());
    std::thread::sleep(Duration::from_millis(300));
    assert!(player.position() > Duration::ZERO, "播放后位置应前进");
    assert!(player.buffered_frames() > 0, "应有预解码缓冲");

    // 暂停
    player.pause();
    let pos_at_pause = player.position();
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        player.position().saturating_sub(pos_at_pause) < Duration::from_millis(30),
        "暂停后位置应基本不动"
    );

    // 继续
    player.play();
    std::thread::sleep(Duration::from_millis(150));
    assert!(player.position() > pos_at_pause, "恢复后位置应继续前进");

    // 按时间 Seek
    player.seek_time(Duration::from_secs(3)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let pos_after_seek = player.position();
    assert!(
        pos_after_seek >= Duration::from_secs(2) && pos_after_seek <= Duration::from_secs(4),
        "seek 后位置应在目标附近，实际 {pos_after_seek:?}"
    );

    // 按百分比 Seek
    player.seek_percent(0.8).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let pos_pct = player.position();
    assert!(
        pos_pct >= Duration::from_secs(3),
        "80% seek 后应接近尾部，实际 {pos_pct:?}"
    );

    // 音量 / 声像
    player.set_volume(0.5);
    assert!((player.volume() - 0.5).abs() < 1e-6);
    player.set_pan(-1.0);
    assert!((player.pan() + 1.0).abs() < 0.01);
    player.set_pan(2.0); // 越界 clamp
    assert!((player.pan() - 1.0).abs() < 0.01);

    // on_end 回调（seek 到尾部 + 播放到底）
    let end_count = Arc::new(AtomicUsize::new(0));
    let ec = end_count.clone();
    player.set_on_end(move || {
        ec.fetch_add(1, Ordering::SeqCst);
    });
    player.seek_time(total_dur - Duration::from_millis(200)).unwrap();
    player.play();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(end_count.load(Ordering::SeqCst) >= 1, "应触发一次 on_end");

    let _ = std::fs::remove_file(&path);
}

/// 裸句柄两步式用法：`Player::new()` 建空句柄，再 `load_file` / `play_file` 加载。
/// 并验证换源（不同采样率）时保存的句柄依然有效、EQ 按新采样率重算、用户设置跨源保留。
#[test]
fn bare_handle_and_cross_source_reload() {
    if !has_output_device() {
        eprintln!("跳过：无默认输出设备");
        return;
    }

    let dir = std::env::temp_dir();
    let path_44k = dir.join("audio_lib_test_reload_44k.wav");
    let path_48k = dir.join("audio_lib_test_reload_48k.wav");
    make_wav(&path_44k, 44_100, 2.0);
    make_wav(&path_48k, 48_000, 2.0);

    // 两步式：先建空句柄（未加载，采样率为 0），再加载。
    let player = Player::new().expect("new");
    assert_eq!(player.sample_rate(), 0, "未加载时采样率应为 0");
    assert!(player.is_paused(), "未加载时处于暂停态");
    assert_eq!(player.duration(), None, "未加载无总时长");

    // 加载前就能配置用户设置（这些跨源保留）。
    let eq = player.eq();
    let fade = player.fade();
    eq.set_band_gain(5, 6.0);
    fade.set_fade_in(Duration::from_millis(100));

    // 加载第一个源（44.1kHz），保持暂停。
    player.load_file(&path_44k).expect("load 44k");
    assert_eq!(player.sample_rate(), 44_100);
    assert!(player.is_paused(), "加载后应保持暂停");
    assert!(player.duration().is_some());

    // 加载前设置的 EQ 增益在加载后依然生效（跨源保留）。
    assert!(eq.is_enabled());
    assert_eq!(eq.band_gain(5), Some(6.0), "EQ 增益应跨源保留");

    // 换到第二个源（48kHz）：采样率变，EQ 应按新采样率重算，增益保留。
    player.load_file(&path_48k).expect("load 48k");
    assert_eq!(player.sample_rate(), 48_000);
    assert_eq!(eq.sample_rate(), 48_000, "EQ 应感知新采样率");
    assert_eq!(eq.band_gain(5), Some(6.0), "换源后增益应保留");
    assert!((fade.fade_in_duration() - Duration::from_millis(100)).is_zero());

    // 换源后 play_file 立即播放（加载 + 播放一步到位）。
    player.play_file(&path_44k).expect("play 44k");
    assert!(!player.is_paused(), "play_file 后应在播放");
    assert_eq!(player.sample_rate(), 44_100);
    std::thread::sleep(Duration::from_millis(200));
    assert!(player.position() > Duration::ZERO, "play_file 应真正发声前进了位置");

    let _ = std::fs::remove_file(&path_44k);
    let _ = std::fs::remove_file(&path_48k);
}
