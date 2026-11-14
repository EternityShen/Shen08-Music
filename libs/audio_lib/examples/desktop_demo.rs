//! 桌面端命令行演示：播放一个音频文件，展示播放/暂停/Seek/音量/声像/结束回调。
//!
//! 用法：`cargo run --release --example desktop_demo -- /path/to/song.mp3`
//!
//! 按键控制：空格=暂停/继续，←/→=快退/快进 5 秒，↑/↓=音量，q=退出。
//! （简单起见用行输入模拟控制，真实 GUI 里把这些方法接到你的 UI 事件即可。）

use std::io::{BufRead, Write};
use std::time::Duration;

use audio_lib::{Player, SeekTo};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("用法: {} <音频文件或URL>", std::env::args().next().unwrap());
        std::process::exit(1);
    });

    let player = if arg.starts_with("http://") || arg.starts_with("https://") {
        Player::open_url(&arg)?
    } else {
        Player::open(&arg)?
    };
    println!("已加载: {arg}");
    println!("  采样率: {} Hz", player.sample_rate());
    println!("  声道数: {}", player.channels());
    println!("  总时长: {:?}", player.duration());

    // 标签信息
    let meta = player.metadata();
    if !meta.is_empty() {
        println!("  标签:");
        if let Some(t) = &meta.title { println!("    标题: {t}"); }
        if let Some(a) = &meta.artist { println!("    艺术家: {a}"); }
        if let Some(al) = &meta.album { println!("    专辑: {al}"); }
        if let Some(y) = meta.year { println!("    年份: {y}"); }
    }

    // 淡入淡出各 0.5 秒，播放/暂停自动平滑（避免爆音）
    player.fade().set_fade_in(Duration::from_millis(500), player.sample_rate());
    player.fade().set_fade_out(Duration::from_millis(500), player.sample_rate());

    // 播放结束回调（在独立线程触发，可安全做 UI 投递）
    player.set_on_end(|| println!("\n[事件] 播放结束"));

    player.play();
    println!("\n输入命令: play / pause / toggle / seek<秒> / pct<0-100> / vol<0-100> / pan<-100-100>");
    println!("          eq<段0-9>;<dB> / eqflat / fadein<毫秒> / fadeout<毫秒> / capture / pos / quit");

    let stdin = std::io::stdin();
    loop {
        print!("\n[{:?} / {:?}] > ", player.position(), player.duration());
        std::io::stdout().flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));

        match cmd {
            "play" => player.play(),
            "pause" => player.pause(),
            "toggle" => {
                let playing = player.toggle_play_pause();
                println!("-> {}", if playing { "播放" } else { "暂停" });
            }
            "seek" => match arg.parse::<f64>() {
                Ok(secs) => {
                    player.seek_time(Duration::from_secs_f64(secs))?;
                    println!("-> 跳到 {secs}s");
                }
                Err(_) => println!("用法: seek <秒>"),
            },
            "pct" => match arg.parse::<f64>() {
                Ok(p) => {
                    player.seek(SeekTo::Percent(p / 100.0))?;
                    println!("-> 跳到 {p}%");
                }
                Err(_) => println!("用法: pct <0-100>"),
            },
            "vol" => match arg.parse::<f64>() {
                Ok(v) => {
                    player.set_volume((v / 100.0) as f32);
                    println!("-> 音量 {}", player.volume());
                }
                Err(_) => println!("用法: vol <0-100>"),
            },
            "pan" => match arg.parse::<f64>() {
                Ok(p) => {
                    player.set_pan((p / 100.0) as f32);
                    println!("-> 声像 {}", player.pan());
                }
                Err(_) => println!("用法: pan <-100-100>"),
            },
            "pos" => println!(
                "位置 {:?} / {:?}",
                player.position(),
                player.duration()
            ),
            // EQ：eq<段0-9>;<dB>，如 `eq5;3` 把 1kHz 段提 3dB
            "eq" => match arg.split_once(';').and_then(|(b, g)| {
                Some((b.parse::<usize>().ok()?, g.parse::<f32>().ok()?))
            }) {
                Some((band, db)) => {
                    player.eq().set_band_gain(band, db);
                    let f = player.eq().band_freq(band).unwrap_or(0.0);
                    println!("-> 第 {band} 段（{f}Hz）设 {db:+}dB");
                }
                None => println!("用法: eq <段0-9>;<dB>"),
            },
            // 所有段归零
            "eqflat" => {
                player.eq().set_gains(&[0.0; 10]);
                println!("-> EQ 已归零");
            },
            // 手动设淡入时长（毫秒）
            "fadein" => match arg.parse::<u64>() {
                Ok(ms) => {
                    player.fade().set_fade_in(Duration::from_millis(ms), player.sample_rate());
                    println!("-> 淡入 {}ms", ms);
                }
                Err(_) => println!("用法: fadein <毫秒>"),
            },
            "fadeout" => match arg.parse::<u64>() {
                Ok(ms) => {
                    player.fade().set_fade_out(Duration::from_millis(ms), player.sample_rate());
                    println!("-> 淡出 {}ms", ms);
                }
                Err(_) => println!("用法: fadeout <毫秒>"),
            },
            // 捕获 1 秒输出，打印电平峰值
            "capture" => {
                let cap = player.enable_capture();
                println!("-> 捕获已开启 1 秒...");
                std::thread::sleep(Duration::from_secs(1));
                let mut peak = 0.0f32;
                let mut total = 0usize;
                let mut tmp = [0f32; 4096];
                loop {
                    let n = cap.pop(&mut tmp);
                    if n == 0 {
                        break;
                    }
                    total += n;
                    for &s in &tmp[..n] {
                        peak = peak.max(s.abs());
                    }
                }
                player.disable_capture();
                println!("-> 捕获 {total} 采样，峰值 {peak:.4}");
            },
            "quit" | "q" => break,
            "" => {}
            other => println!("未知命令: {other}"),
        }
    }

    println!("退出。");
    Ok(())
}
