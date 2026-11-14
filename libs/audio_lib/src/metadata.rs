//! 标签信息（metadata）提取。
//!
//! 用 Symphonia 自带的 metadata reader 从容器里读标准标签，无需额外依赖。
//! 打开文件时一次性提取，存到 [`Player`] 上，UI 随时可取。
//!
//! 覆盖常见字段：标题、艺术家、专辑、流派、年份、音轨号、时长等。
//! 各容器（ID3 / Vorbis Comment / MP4 iTunes）都会映射到
//! Symphonia 的 `StandardTagKey`，所以一套代码通吃 mp3 / flac / m4a / ogg。

use symphonia::core::meta::{StandardTagKey, Value};

/// 一首歌的标签信息。字段缺失为 `None`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMetadata {
    /// 标题（track title）。
    pub title: Option<String>,
    /// 主艺术家。
    pub artist: Option<String>,
    /// 专辑名。
    pub album: Option<String>,
    /// 专辑艺术家（合辑里区分）。
    pub album_artist: Option<String>,
    /// 流派。
    pub genre: Option<String>,
    /// 发行年份 / 日期（取 `Date` 标签，尽量截成年份）。
    pub year: Option<u32>,
    /// 音轨号。
    pub track_number: Option<u32>,
    /// 专辑内总音轨数。
    pub track_total: Option<u32>,
    /// 碟号。
    pub disc_number: Option<u32>,
    /// 作曲者。
    pub composer: Option<String>,
    /// 备注 / 评论。
    pub comment: Option<String>,
}

impl TrackMetadata {
    /// 从一段 Symphonia metadata revision 的标签列表提取。
    ///
    /// 同名标签多条时取第一条；`year` 同时尝试 `Date` 和 `EncodingDate`。
    pub fn from_tags(tags: &[symphonia::core::meta::Tag]) -> Self {
        let mut m = Self::default();
        for tag in tags {
            let Some(std_key) = tag.std_key else {
                continue; // 只关心能映射到标准键的标签
            };
            let val = &tag.value;
            match std_key {
                StandardTagKey::TrackTitle => m.title = string_of(val),
                StandardTagKey::Artist => m.artist = string_of(val),
                StandardTagKey::Album => m.album = string_of(val),
                StandardTagKey::AlbumArtist => m.album_artist = string_of(val),
                StandardTagKey::Genre => m.genre = string_of(val),
                StandardTagKey::Composer => m.composer = string_of(val),
                StandardTagKey::Comment => m.comment = string_of(val),
                StandardTagKey::TrackNumber => m.track_number = uint_of(val),
                StandardTagKey::TrackTotal => m.track_total = uint_of(val),
                StandardTagKey::DiscNumber => m.disc_number = uint_of(val),
                // Date 可能是 "2020" 也可能 "2020-05-01"，取前 4 位年份。
                // EncodingDate 仅在 Date 没给年份时兜底；year 已有值则不覆盖。
                StandardTagKey::Date | StandardTagKey::EncodingDate if m.year.is_none() => {
                    m.year = year_of(val);
                }
                _ => {}
            }
        }
        m
    }

    /// 是否一个标签都没读到。
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// 把 tag value 尽量转成字符串。
fn string_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::UnsignedInt(n) => Some(n.to_string()),
        Value::SignedInt(n) => Some(n.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 把 tag value 尽量转成无符号整数。
fn uint_of(v: &Value) -> Option<u32> {
    match v {
        Value::UnsignedInt(n) => u32::try_from(*n).ok(),
        Value::SignedInt(n) => u32::try_from(*n).ok(),
        Value::String(s) => {
            // "3/12" 这种取斜杠前的数字
            let head = s.split('/').next()?.trim();
            head.parse().ok()
        }
        _ => None,
    }
}

/// 从 tag value 里抽 4 位年份。
fn year_of(v: &Value) -> Option<u32> {
    let s = match v {
        Value::String(s) => s.as_str(),
        Value::UnsignedInt(n) => return u32::try_from(*n).ok(),
        _ => return None,
    };
    // 取前 4 个数字字符。容忍 "2020-05-01" / "20200501" / "2020" 等格式。
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
    if digits.len() == 4 {
        digits.parse().ok()
    } else {
        None
    }
}
