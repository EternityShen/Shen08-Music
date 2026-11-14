//! URL 播放集成测试：内置一个支持 HTTP Range 的极简静态服务器，
//! 让 `Player::open_url` 走完整的 网络 -> HttpSource -> 解码 -> 环形缓冲 -> 回调 链路。
//!
//! 服务器只用 std::net 手写，避免额外依赖，也能离线跑。

// 整个文件依赖 `url` feature（HttpSource / load_url）。关掉该 feature 时不编译，
// 这样 `cargo test --no-default-features --all-targets` 也能通过。
#![cfg(feature = "url")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use audio_lib::Player;
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

/// 处理单个连接：解析请求行 + Range 头，回 200 或 206。
fn handle_client(mut stream: TcpStream, body: &[u8]) {
    let mut buf = [0u8; 4096];
    let mut req = Vec::new();
    // 读到请求头结束（空行）
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return,
        };
        req.extend_from_slice(&buf[..n]);
        if req.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if req.len() > 64 * 1024 {
            return;
        }
    }
    let text = String::from_utf8_lossy(&req);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let _path = parts.next().unwrap_or("/").to_string();

    let mut range_start: Option<u64> = None;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("range:") {
            // 形如 bytes=123-
            if let Some(eq) = rest.find('=') {
                let val = rest[eq + 1..].trim();
                if let Some(dash) = val.find('-') {
                    range_start = val[..dash].trim().parse().ok();
                }
            }
        }
    }

    let (status, extra, slice) = match range_start {
        Some(start) if start < body.len() as u64 => {
            let s = start as usize;
            let end = body.len() - 1;
            (
                "206 Partial Content",
                format!(
                    "Content-Range: bytes {}-{}/{}\r\n",
                    start, end, body.len()
                ),
                &body[s..],
            )
        }
        Some(_) => (
            "416 Range Not Satisfiable",
            format!("Content-Range: bytes */{}\r\n", body.len()),
            &body[0..0],
        ),
        None => ("200 OK", String::new(), body),
    };

    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n{extra}Connection: close\r\n\r\n",
        slice.len()
    );
    let _ = stream.write_all(head.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(slice);
    }
    let _ = stream.flush();
}

/// 启动一个一次性静态服务器，返回监听地址。每个连接独立处理（支持连接关闭后重连）。
fn spawn_server(body: Arc<Vec<u8>>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::Builder::new()
        .name("test-http-server".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let body = body.clone();
                // 每个连接一个线程，容忍 ureq 的连接复用/重连
                std::thread::spawn(move || handle_client(stream, &body));
            }
        })
        .unwrap();
    addr
}

#[test]
fn url_playback_and_seek() {
    if !cpal::default_host().default_output_device().is_some() {
        eprintln!("跳过：无默认输出设备");
        return;
    }

    let dir = std::env::temp_dir();
    let path = dir.join("audio_lib_test_url.wav");
    let total = make_wav(&path, 44_100, 4.0);
    let bytes = std::fs::read(&path).expect("read wav");
    let total_dur = Duration::from_secs_f64(total as f64 / 44_100.0);

    let addr = spawn_server(Arc::new(bytes));
    let url = format!("http://{addr}/tone.wav");

    let player = Player::open_url(&url).expect("open url");
    assert_eq!(player.sample_rate(), 44_100);
    assert_eq!(player.channels(), 2);
    assert_eq!(player.duration(), Some(total_dur), "应从 HTTP 头拿到总时长");
    assert!(player.is_paused());

    // 播放
    player.play();
    std::thread::sleep(Duration::from_millis(400));
    assert!(player.position() > Duration::ZERO, "URL 播放位置应前进");

    // 按时间 Seek（触发中断 + 重新 Range 拉取）
    player.seek_time(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let pos = player.position();
    assert!(
        pos >= Duration::from_secs(1) && pos <= Duration::from_secs(3),
        "URL seek 后位置应在目标附近，实际 {pos:?}"
    );

    // 按百分比 Seek
    player.seek_percent(0.9).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let pos2 = player.position();
    assert!(
        pos2 >= Duration::from_secs(3),
        "90% seek 后应接近尾部，实际 {pos2:?}"
    );

    // on_end：跳到尾部播放到底
    let end_count = Arc::new(AtomicUsize::new(0));
    let ec = end_count.clone();
    player.set_on_end(move || {
        ec.fetch_add(1, Ordering::SeqCst);
    });
    player.seek_time(total_dur - Duration::from_millis(200)).unwrap();
    player.play();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(end_count.load(Ordering::SeqCst) >= 1, "URL 播放应触发 on_end");

    let _ = std::fs::remove_file(&path);
}
