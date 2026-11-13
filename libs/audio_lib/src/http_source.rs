//! HTTP(S) 流式 `MediaSource` 实现（`url` feature）。
//!
//! 核心思路：一个后台线程用 HTTP Range 顺序拉取字节，写进一个共享的「向前增长」缓冲；
//! 解码线程（Symphonia）从缓冲里读。这样解码线程永远不会阻塞在网络往返上，
//! Seek 也只需移动读位置 / 触发一次新的 Range 请求。
//!
//! 缓冲语义：
//! - `buffer` 覆盖文件的绝对字节区间 `[buffer_start, buffer_start + buffer.len())`。
//! - 读取只向后推进 `pos`。当 `pos` 落在缓冲内就直接读；落在缓冲之后就让网络线程追上来（阻塞等待）。
//! - Seek 到缓冲之外（通常是往回跳，或大幅前跳）时，丢弃现有流、从新位置重新发 Range 请求。
//! - 为限制内存，缓冲头部落后 `pos` 太多时逐步裁剪。

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use symphonia::core::io::MediaSource;

/// 缓冲最多保留多少字节（约 2 MiB，足够几十秒音频的预读）。
const MAX_BUFFER_BYTES: usize = 2 << 20;
/// 缓冲满了 / 没流时，后台线程短暂 park 的时长。
const FETCH_PARK: Duration = Duration::from_millis(10);
/// 单次从网络读入缓冲的块大小。
const READ_CHUNK: usize = 32 * 1024;

/// 对某个 HTTP 资源发起一次（Range）请求，返回只读字节流。
///
/// 抽象成 trait 便于以后替换后端（reqwest / 自建 HLS / 本地缓存层等）。
pub trait HttpFetcher: Send + Sync + 'static {
    /// 发起一次请求，从 `range_start` 字节开始读。
    ///
    /// 返回 `(reader, total_size, range_supported)`：
    /// - `total_size`：资源总字节数，未知为 `None`。
    /// - `range_supported`：服务器是否返回 206（支持 Range）。
    fn fetch(&self, range_start: u64) -> io::Result<(Box<dyn Read + Send>, Option<u64>, bool)>;
}

/// 基于 ureq 的 [`HttpFetcher`] 实现。
pub struct UreqFetcher {
    agent: ureq::Agent,
    url: String,
}

impl UreqFetcher {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            agent: ureq::agent(),
            url: url.into(),
        }
    }
}

impl HttpFetcher for UreqFetcher {
    fn fetch(&self, range_start: u64) -> io::Result<(Box<dyn Read + Send>, Option<u64>, bool)> {
        let mut req = self.agent.get(&self.url);
        if range_start > 0 {
            req = req.header("Range", &format!("bytes={range_start}-"));
        }
        let resp = req.call().map_err(io::Error::other)?;
        let status = resp.status();

        // 非正常响应统一当 IO 错误抛给 Symphonia。
        if !(status == ureq::http::StatusCode::OK
            || status == ureq::http::StatusCode::PARTIAL_CONTENT)
        {
            return Err(io::Error::other(format!("unexpected HTTP status: {status}")));
        }

        let range_ok = status == ureq::http::StatusCode::PARTIAL_CONTENT;
        let total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|len| if range_ok { len + range_start } else { len });

        let reader = resp.into_body().into_reader();
        Ok((Box::new(reader), total, range_ok))
    }
}

/// 解码线程与网络线程共享的状态。
struct HttpShared {
    /// 已下载的字节，覆盖绝对区间 `[buffer_start, buffer_start + buffer.len())`。
    buffer: Mutex<HttpBuffer>,
    /// 缓冲变化 / seek 请求 / EOF 的唤醒器。
    cv: Condvar,
    /// 读中断：Seek 时置位，让阻塞在 read() 上的解码线程立刻返回。
    interrupt: Arc<AtomicBool>,
    /// 后台线程退出标志（Drop HttpSource 时置位）。
    shutdown: AtomicBool,
}

struct HttpBuffer {
    data: Vec<u8>,
    buffer_start: u64,
    /// 读游标（文件绝对字节位置）。
    pos: u64,
    /// 网络流是否已到末尾。
    eof: bool,
    /// 资源总大小，未知为 0。
    total: u64,
    /// 服务器是否支持 Range。
    range_ok: bool,
}

impl HttpShared {
    /// 等待 `pos` 处至少有一个字节可读，或 EOF / 中断。返回是否可读。
    fn wait_available(&self, pos: u64) -> bool {
        let mut buf = self.buffer.lock().unwrap();
        loop {
            if self.interrupt.load(Ordering::Relaxed) {
                return false;
            }
            let in_range = pos >= buf.buffer_start
                && pos < buf.buffer_start + buf.data.len() as u64;
            if in_range || buf.eof {
                return in_range;
            }
            let (b, _) = self
                .cv
                .wait_timeout(buf, FETCH_PARK)
                .unwrap_or_else(|e| e.into_inner());
            buf = b;
        }
    }

    fn read(&self, out: &mut [u8]) -> usize {
        let mut buf = self.buffer.lock().unwrap();
        if buf.pos < buf.buffer_start || buf.pos >= buf.buffer_start + buf.data.len() as u64 {
            return 0;
        }
        let off = (buf.pos - buf.buffer_start) as usize;
        let n = out.len().min(buf.data.len() - off);
        out[..n].copy_from_slice(&buf.data[off..off + n]);
        buf.pos += n as u64;
        n
    }

    /// 移动读游标到 `new_pos`。若目标不在缓冲内，丢弃现有流并从新位置重新拉取。
    fn seek(&self, new_pos: u64) {
        let mut buf = self.buffer.lock().unwrap();
        buf.pos = new_pos;
        let in_buffer =
            new_pos >= buf.buffer_start && new_pos < buf.buffer_start + buf.data.len() as u64;
        if !in_buffer {
            // 丢弃现有流：后台线程发现 buffer_start 落后 pos 会重新发 Range 请求。
            buf.data.clear();
            buf.buffer_start = new_pos;
            buf.eof = false;
            self.cv.notify_all();
        }
    }
}

/// Symphonia 的 `MediaSource` 实现：从共享缓冲读取 HTTP 字节流。
pub struct HttpSource {
    shared: Arc<HttpShared>,
}

impl HttpSource {
    /// 打开一个 HTTP(S) 音频资源。
    ///
    /// 立刻返回并启动后台拉取线程；`MediaSourceStream` 的前几次 read 会阻塞
    /// 到首块数据到达（Symphonia 探测容器头需要前几十 KB）。
    pub fn new(url: impl Into<String>) -> io::Result<Self> {
        let url = url.into();
        let shared = Arc::new(HttpShared {
            buffer: Mutex::new(HttpBuffer {
                data: Vec::new(),
                buffer_start: 0,
                pos: 0,
                eof: false,
                total: 0,
                range_ok: true,
            }),
            cv: Condvar::new(),
            interrupt: Arc::new(AtomicBool::new(false)),
            shutdown: AtomicBool::new(false),
        });

        let fetcher = UreqFetcher::new(url);
        let fetch_shared = shared.clone();
        std::thread::Builder::new()
            .name("audio-lib-http".into())
            .spawn(move || fetch_loop(fetch_shared, fetcher))
            .map_err(|e| io::Error::other(e.to_string()))?;

        Ok(Self { shared })
    }

    /// 拿到中断句柄，交给 Symphonia / 控制线程，用于打断阻塞中的 read。
    pub fn interrupt_handle(&self) -> Arc<AtomicBool> {
        self.shared.interrupt.clone()
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        self.shared.cv.notify_all();
    }
}

impl Read for HttpSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let pos = {
            let buf = self.shared.buffer.lock().unwrap();
            buf.pos
        };
        if !self.shared.wait_available(pos) {
            // 中断或 EOF
            if self.shared.interrupt.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "seek interrupt"));
            }
            return Ok(0);
        }
        Ok(self.shared.read(out))
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let (total, cur) = {
            let buf = self.shared.buffer.lock().unwrap();
            (buf.total, buf.pos)
        };
        let new_pos = match to {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(d) => (cur as i128 + d as i128).max(0) as u64,
            SeekFrom::End(d) => {
                let Some(total) = (total > 0).then_some(total) else {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "stream length unknown",
                    ));
                };
                (total as i128 + d as i128).max(0) as u64
            }
        };
        self.shared.seek(new_pos);
        Ok(new_pos)
    }
}

impl MediaSource for HttpSource {
    fn is_seekable(&self) -> bool {
        // 只要服务器支持 Range 就可 Seek；否则只能顺序播放。
        self.shared.buffer.lock().unwrap().range_ok
    }

    fn byte_len(&self) -> Option<u64> {
        let buf = self.shared.buffer.lock().unwrap();
        (buf.total > 0).then_some(buf.total)
    }
}

/// 后台拉取循环：顺序读网络、写入共享缓冲、按需裁剪、响应 Seek 重连。
fn fetch_loop(shared: Arc<HttpShared>, fetcher: impl HttpFetcher) {
    let mut stream: Option<Box<dyn Read + Send>> = None;
    let mut read_total: Option<u64> = None;

    loop {
        if shared.shutdown.load(Ordering::Relaxed) {
            return;
        }

        let (need_reconnect, target) = {
            let buf = shared.buffer.lock().unwrap();
            let buffered_end = buf.buffer_start + buf.data.len() as u64;
            let reconnect = buf.pos < buf.buffer_start
                || (buf.pos > buffered_end && !buf.eof)
                || (buf.pos != buf.buffer_start && buf.data.is_empty() && !buf.eof);
            (reconnect, buf.pos)
        };

        if need_reconnect || stream.is_none() {
            match fetcher.fetch(target) {
                Ok((reader, total, range_ok)) => {
                    let mut buf = shared.buffer.lock().unwrap();
                    buf.data.clear();
                    buf.buffer_start = target;
                    buf.eof = false;
                    if let Some(t) = total {
                        buf.total = t;
                    }
                    buf.range_ok = range_ok;
                    drop(buf);
                    stream = Some(reader);
                    read_total = Some(0);
                    shared.cv.notify_all();
                }
                Err(e) => {
                    // 网络/服务器错误：短暂退避后重试，避免忙转。
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
            }
        }

        let Some(reader) = stream.as_mut() else {
            continue;
        };

        // 缓冲尾部若已落后读位置很多，先裁剪头部释放空间。
        {
            let mut buf = shared.buffer.lock().unwrap();
            if buf.data.len() >= MAX_BUFFER_BYTES {
                let keep_from = (buf.pos - buf.buffer_start) as usize;
                buf.data.drain(..keep_from);
                buf.buffer_start = buf.pos;
            }
        }

        // 先在锁外读到临时块，再短暂加锁追加，避免长时间持锁。
        let mut chunk = vec![0u8; READ_CHUNK];
        let n = match reader.read(&mut chunk) {
            Ok(0) => {
                let mut buf = shared.buffer.lock().unwrap();
                buf.eof = true;
                drop(buf);
                shared.cv.notify_all();
                stream = None;
                continue;
            }
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                // 连接断了：下次循环会重连。
                stream = None;
                continue;
            }
        };

        {
            // 追加到缓冲尾部。注意上面的裁剪可能已让 buffer_start 前移，
            // 但尾部（buffer_start + data.len()）始终是这次 read 的续写点。
            let mut buf = shared.buffer.lock().unwrap();
            buf.data.extend_from_slice(&chunk[..n]);
        }
        read_total = read_total.map(|t| t + n as u64);
        let _ = read_total;
        shared.cv.notify_all();
    }
}
