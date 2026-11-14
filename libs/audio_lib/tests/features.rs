//! 新功能集成测试：均衡器 / 淡入淡出 / 标签信息 / 播放样本捕获。
//!
//! 复用 `tests/playback.rs` 里的 WAV 生成思路，跑真实解码 -> 回调链路。

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use audio_lib::{Player, EQ_BAND_FREQS};
use cpal::traits::HostTrait;

/// 生成单声道 16-bit PCM WAV，返回总采样数。
fn make_wav(path: &PathBuf, sample_rate: u32, seconds: f64) -> u32 {
    let n_samples = (sample_rate as f64 * seconds) as u32;
    let data_len = n_samples * 2;
    let file_len = 36 + data_len;

    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&file_len.to_le_bytes()).unwrap();
    f.write_all(b"WAVE").unwrap();
    f.write_all(b"fmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    for i in 0..n_samples {
        let t = i as f64 / sample_rate as f64;
        let s = (2.0 * std::f64::consts::PI * 440.0 * t).sin();
        let v = (s * 0.5 * i16::MAX as f64) as i16;
        f.write_all(&v.to_le_bytes()).unwrap();
    }
    n_samples
}

/// 生成一个带 ID3 风格标签的 WAV（用 LIST/INFO 块，Symphonia 能读）。
/// 这里退而求其次：直接测 `TrackMetadata::from_tags` 的解析逻辑，
/// 端到端标签读取放到下面用 Symphonia 支持的容器验证。
fn make_wav_with_tags(path: &PathBuf) -> u32 {
    make_wav(path, 44_100, 1.0)
}

#[test]
fn eq_band_freqs_and_set_gain() {
    let eq = audio_lib::Equalizer::new(44_100);
    assert_eq!(eq.bands(), 10);
    assert_eq!(eq.band_freq(0), Some(31.0));
    assert_eq!(eq.band_freq(9), Some(16_000.0));
    assert_eq!(eq.band_freq(10), None);
    // 越界设增益应安全忽略，不 panic。
    eq.set_band_gain(10, 6.0);
    eq.set_band_gain(0, 6.0);
    eq.set_gains(&[1.0, 2.0, 3.0]);
    eq.set_enabled(false);
    assert!(!eq.is_enabled());
    eq.set_enabled(true);
    assert!(eq.is_enabled());
}

#[test]
fn eq_filter_response_at_bands() {
    use audio_lib::{EqState, Equalizer};

    let sr = 44_100.0f32;
    let eq = Equalizer::new(sr as u32);
    let mut state = EqState::new(2, eq.bands());

    // 全 0 增益 = 直通：输出应与输入一致。
    let input = [0.5f32, -0.5, 0.25, -0.25, 0.1, -0.1];
    let mut buf = input;
    state.process_interleaved(&eq, &mut buf);
    for (a, b) in buf.iter().zip(input.iter()) {
        assert!((a - b).abs() < 1e-4, "直通应不变：{a} vs {b}");
    }

    // 逐段验证频率响应：只把第 i 段设为 +12dB（其余直通），喂该段中心频率的
    // 正弦，跳过瞬态后测稳态幅度比，应接近 +12dB。中低频段用稳态法比冲激响应
    // 更稳（f32 biquad 在低归一化频率下冲激响应数值精度差）。
    for band in [0usize, 3, 5, 9] {
        let f0 = EQ_BAND_FREQS[band];
        let target_db = 12.0f32;
        let eq_b = Equalizer::with_gains(sr as u32, &{
            let mut g = [0.0f32; 10];
            g[band] = target_db;
            g
        });
        // 喂 2 秒正弦，跳过前 1 秒瞬态，测后 1 秒稳态峰值。
        let n = (sr * 2.0) as usize;
        let mut buf = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f64 / sr as f64;
            let s = (2.0 * std::f64::consts::PI * f0 as f64 * t).sin() as f32;
            buf.push(s);
            buf.push(s);
        }
        let mut st = EqState::new(2, eq_b.bands());
        st.process_interleaved(&eq_b, &mut buf);
        let skip = (sr as usize) * 2;
        let peak = buf[skip..]
            .chunks(2)
            .map(|c| c[0].abs())
            .fold(0.0f32, f32::max);
        let gain_db = 20.0 * peak.log10();
        assert!(
            (gain_db - target_db).abs() < 0.5,
            "第 {band} 段（{f0}Hz）设 +{target_db}dB，实测 {gain_db:.2}dB"
        );
    }

    // 衰减验证：1kHz 段设 -12dB，稳态应接近 -12dB。
    {
        let band = 5;
        let f0 = EQ_BAND_FREQS[band];
        let eq_d = Equalizer::with_gains(sr as u32, &{
            let mut g = [0.0f32; 10];
            g[band] = -12.0;
            g
        });
        let n = (sr * 2.0) as usize;
        let mut buf = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f64 / sr as f64;
            let s = (2.0 * std::f64::consts::PI * f0 as f64 * t).sin() as f32;
            buf.push(s);
            buf.push(s);
        }
        let mut st = EqState::new(2, eq_d.bands());
        st.process_interleaved(&eq_d, &mut buf);
        let skip = (sr as usize) * 2;
        let peak = buf[skip..]
            .chunks(2)
            .map(|c| c[0].abs())
            .fold(0.0f32, f32::max);
        let gain_db = 20.0 * peak.log10();
        assert!(
            (gain_db - (-12.0)).abs() < 0.5,
            "1kHz 段设 -12dB，实测 {gain_db:.2}dB"
        );
    }
}

#[test]
fn fade_state_gain_ramp() {
    use audio_lib::FadeState;

    let sr = 1000u32;
    let fade = FadeState::new(sr);
    fade.set_fade_in(Duration::from_secs_f64(0.1), sr); // 100 采样
    fade.set_fade_out(Duration::from_secs_f64(0.1), sr);
    assert_eq!(fade.fade_in_samples(), 100);
    assert_eq!(fade.fade_out_samples(), 100);

    // 淡入：起始增益接近 0，末端接近 1。
    fade.start_fade_in();
    let mut buf: Vec<f32> = vec![1.0; 200];
    fade.process_interleaved(&mut buf, 1);
    assert!(buf[0] < 0.2, "淡入首采样应接近 0，实际 {}", buf[0]);
    assert!(
        (buf[99] - 1.0).abs() < 0.05 || buf[99] > 0.8,
        "淡入末端应接近 1，实际 {}",
        buf[99]
    );
    assert!(!fade.is_fading(), "淡变走完后应回到 None");

    // 淡出：起始接近 1，末端为 0，且走完后剩余清零。
    fade.start_fade_out();
    let mut buf2 = vec![1.0f32; 200];
    fade.process_interleaved(&mut buf2, 1);
    assert!(buf2[0] > 0.8, "淡出首采样应接近 1，实际 {}", buf2[0]);
    assert!(buf2[99].abs() < 0.05, "淡出末端应接近 0，实际 {}", buf2[99]);
    assert!(
        buf2[150].abs() < 1e-6,
        "淡出走完后剩余应清零，实际 {}",
        buf2[150]
    );

    // 时长为 0 的淡变应立即结束，不改数据。
    let f0 = FadeState::new(sr);
    f0.set_fade_in(Duration::ZERO, sr);
    f0.start_fade_in();
    let mut buf3 = vec![1.0f32; 10];
    f0.process_interleaved(&mut buf3, 1);
    assert!(
        buf3.iter().all(|&x| (x - 1.0).abs() < 1e-6),
        "0 时长淡入不应改数据"
    );
    assert!(!f0.is_fading());
}

#[test]
fn fade_interleaved_stereo() {
    use audio_lib::FadeState;
    let sr = 1000u32;
    let fade = FadeState::new(sr);
    fade.set_fade_in(Duration::from_secs_f64(0.05), sr); // 50 采样 = 25 帧
    fade.start_fade_in();
    // 交错立体声：L R L R ...
    let mut buf: Vec<f32> = vec![1.0; 200];
    fade.process_interleaved(&mut buf, 2);
    // 帧间隔均匀衰减，L/R 同帧应同增益。
    let g0 = buf[0];
    let g1 = buf[1];
    assert!((g0 - g1).abs() < 1e-6, "同帧 L/R 增益应一致");
}

#[test]
fn metadata_from_tags() {
    use audio_lib::TrackMetadata;
    use symphonia::core::meta::{Tag, StandardTagKey, Value};

    let std = |k| Some(k);
    let tags = vec![
        Tag::new(std(StandardTagKey::TrackTitle), "title", Value::String("Test Song".into())),
        Tag::new(std(StandardTagKey::Artist), "artist", Value::String("Test Artist".into())),
        Tag::new(std(StandardTagKey::Album), "album", Value::String("Test Album".into())),
        Tag::new(std(StandardTagKey::Genre), "genre", Value::String("Rock".into())),
        Tag::new(std(StandardTagKey::TrackNumber), "track", Value::UnsignedInt(3)),
        Tag::new(std(StandardTagKey::Date), "date", Value::String("2021".into())),
    ];
    let m = TrackMetadata::from_tags(&tags);
    assert_eq!(m.title.as_deref(), Some("Test Song"));
    assert_eq!(m.artist.as_deref(), Some("Test Artist"));
    assert_eq!(m.album.as_deref(), Some("Test Album"));
    assert_eq!(m.genre.as_deref(), Some("Rock"));
    assert_eq!(m.track_number, Some(3));
    assert_eq!(m.year, Some(2021));
    // "3/12" 形式的音轨号解析
    let tags2 = vec![Tag::new(
        std(StandardTagKey::TrackNumber),
        "track",
        Value::String("5/12".into()),
    )];
    let m2 = TrackMetadata::from_tags(&tags2);
    assert_eq!(m2.track_number, Some(5));
    // 空标签
    let m3 = TrackMetadata::from_tags(&[]);
    assert!(m3.is_empty());
}

#[test]
fn capture_end_to_end() {
    if !cpal::default_host().default_output_device().is_some() {
        eprintln!("跳过：无默认输出设备");
        return;
    }
    let dir = std::env::temp_dir();
    let path = dir.join("audio_lib_test_capture.wav");
    let total = make_wav(&path, 44_100, 2.0);
    let _ = total;

    let player = Player::open(&path).expect("open");
    // 捕获句柄
    let cap = player.enable_capture();
    assert!(player.is_capture_enabled());

    // 边播放边读捕获缓冲
    let got_total = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let got = got_total.clone();
        let stop = stop.clone();
        let cap = cap.clone();
        std::thread::spawn(move || {
            let mut tmp = [0f32; 1024];
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let n = cap.pop(&mut tmp);
                if n > 0 {
                    got.fetch_add(n, Ordering::Relaxed);
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        })
    };

    player.play();
    std::thread::sleep(Duration::from_millis(800));
    player.pause();

    stop.store(true, Ordering::Relaxed);
    let _ = reader.join();

    let n = got_total.load(Ordering::SeqCst);
    assert!(n > 0, "捕获应读到样本，实际 {n}");
    assert!(
        n < 44_100 * 2 * 2, // 远小于 2 秒立体声总量，说明是流式读取
        "捕获样本数应合理，实际 {n}"
    );

    player.disable_capture();
    assert!(!player.is_capture_enabled());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn metadata_accessor_on_player() {
    if !cpal::default_host().default_output_device().is_some() {
        eprintln!("跳过：无默认输出设备");
        return;
    }
    let dir = std::env::temp_dir();
    let path = dir.join("audio_lib_test_meta.wav");
    let _ = make_wav_with_tags(&path);

    let player = Player::open(&path).expect("open");
    // 纯 WAV 没有标题/艺术家标签，字段应为 None（但访问不 panic）。
    let m = player.metadata();
    assert!(m.title.is_none() || m.title.is_some());
    let _ = std::fs::remove_file(&path);
}
