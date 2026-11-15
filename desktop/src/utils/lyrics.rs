#[derive(Clone, Debug)]
pub struct LyricItem {
    pub time_str: String,
    pub time: f64,
    pub text: String,
}

pub fn create_lrc_lyrics(time: f64, text: String) -> Vec<LyricItem> {
    let mut items: Vec<LyricItem> = Vec::new();
    let mut has_timestamp = false;

    // ---------- 1. 尝试按 LRC 格式解析 ----------
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        // 解析行首可能存在的多个时间戳：[00:12.34][01:23.45]歌词
        // 同时保留原始时间戳文本，供 time_str 使用
        let mut timestamps: Vec<(String, f64)> = Vec::new();
        let mut rest = line;

        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            let inside = &rest[1..end];

            if let Some(t) = parse_timestamp(inside) {
                timestamps.push((inside.trim().to_string(), t));
            }
            // 元数据标签（[ti:] / [ar:] / [al:] / [by:] / [offset:] 等）会被
            // parse_timestamp 返回 None，自然被跳过
            rest = &rest[end + 1..];
        }

        let content = rest.trim().to_string();

        if !timestamps.is_empty() {
            has_timestamp = true;
            for (ts, t) in timestamps {
                items.push(LyricItem {
                    time_str: ts,
                    time: t,
                    text: content.clone(),
                });
            }
        } else if !content.is_empty() {
            // 无时间戳的行先以 -1 占位，稍后统一处理
            items.push(LyricItem {
                time_str: String::new(),
                time: -1.0,
                text: content,
            });
        }
    }

    // ---------- 2. 若存在 LRC 时间戳：过滤占位行、排序后返回 ----------
    if has_timestamp {
        items.retain(|item| item.time >= 0.0);
        items.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        return items;
    }

    // ---------- 3. 否则：纯文本歌词，按总时长均分 ----------
    items.retain(|item| !item.text.is_empty());
    let n = items.len();
    if n == 0 {
        return items;
    }

    // time <= 0 时给一个保底：每行 3 秒
    let step = if time > 0.0 { time / n as f64 } else { 3.0 };

    for (i, item) in items.iter_mut().enumerate() {
        item.time = i as f64 * step;
        item.time_str = format_timestamp(item.time);
    }

    items
}

/// 解析 LRC 时间戳，支持：
/// - `mm:ss`
/// - `mm:ss.xx` / `mm:ss.xxx`（小数秒 / 毫秒都按秒的十进制处理）
/// - `mm:ss:xx`（老式百分秒格式）
fn parse_timestamp(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 {
        return None;
    }

    let minutes: f64 = parts[0].trim().parse().ok()?;

    let seconds: f64 = if parts.len() >= 3 {
        // mm:ss:xx —— xx 是百分秒
        let whole: f64 = parts[1].trim().parse().ok()?;
        let frac: f64 = parts[2].trim().parse().ok()?;
        whole + frac / 100.0
    } else {
        // mm:ss 或 mm:ss.xx
        parts[1].trim().parse().ok()?
    };

    Some(minutes * 60.0 + seconds)
}

/// 把秒数格式化为 `mm:ss.xx`（厘秒精度），用于纯文本均分模式生成 time_str
fn format_timestamp(t: f64) -> String {
    // 先转成整数厘秒，避免浮点误差导致 59.999 被格式化成 "60.00"
    let total_cs = (t.max(0.0) * 100.0).round() as u64;
    let cs = total_cs % 100;
    let total_sec = total_cs / 100;
    let sec = total_sec % 60;
    let min = total_sec / 60;
    format!("{:02}:{:02}.{:02}", min, sec, cs)
}
