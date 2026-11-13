//! 无锁单生产者 / 单消费者（SPSC）环形缓冲。
//!
//! 生产者是解码线程，消费者是 CPAL 的音频回调线程。
//! 音频回调有实时性要求，绝不能阻塞或加锁，因此这里只用原子变量协调，
//! 容量固定（编译期不可变），写入端在缓冲满时自行退避。

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 固定容量的无锁 SPSC 环形缓冲，存储交错的 f32 采样。
///
/// 容量在构造时确定并向上取整到 2 的幂，这样索引可以用位掩码代替取模。
/// `read_pos` 只由消费者写，`write_pos` 只由生产者写，两者都用
/// `Relaxed` 序即可——缓冲数据的可见性由 `Ordering::Release`/`Acquire`
/// 在各自发布/读取时保证。
///
/// 缓冲本体用 [`UnsafeCell`] 做内部可变性：生产者只写 `write_pos` 之后
/// 的区域，消费者只读 `read_pos` 之前的区域，两者不重叠，因此 `Sync`
/// 是安全的。`push` 只能由生产者调用，`pop` 只能由消费者调用。
pub struct RingBuffer {
    buf: UnsafeCell<Box<[f32]>>,
    mask: usize,
    read_pos: AtomicUsize,
    write_pos: AtomicUsize,
}

// SAFETY: 见结构体文档。生产者/消费者分别只触碰互不重叠的内存区域，
// 跨线程协调完全靠 read_pos/write_pos 两个原子变量。
unsafe impl Sync for RingBuffer {}

impl RingBuffer {
    /// 创建容量至少为 `capacity`（向上取整到 2 的幂）的缓冲。
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(2);
        Self {
            buf: UnsafeCell::new(vec![0.0; cap].into_boxed_slice()),
            mask: cap - 1,
            read_pos: AtomicUsize::new(0),
            write_pos: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn read_idx(&self) -> usize {
        self.read_pos.load(Ordering::Relaxed)
    }

    #[inline]
    fn write_idx(&self) -> usize {
        self.write_pos.load(Ordering::Relaxed)
    }

    /// 缓冲中当前可读的采样数。
    #[inline]
    pub fn len(&self) -> usize {
        self.write_idx().wrapping_sub(self.read_idx())
    }

    /// 缓冲是否为空。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 剩余可写空间。
    #[inline]
    pub fn free_space(&self) -> usize {
        // SAFETY: 只读取 slice 长度，不解引用内容。
        (unsafe { &*self.buf.get() }).len() - self.len()
    }

    /// 缓冲总容量。
    #[inline]
    pub fn capacity(&self) -> usize {
        (unsafe { &*self.buf.get() }).len()
    }

    /// 尝试把 `src` 中的采样推入缓冲，返回实际写入的数量。
    ///
    /// 满了就只写一部分（或写 0），绝不覆盖未读数据。生产者负责在
    /// 写入量小于请求量时退避重试。
    pub fn push(&self, src: &[f32]) -> usize {
        let write = self.write_idx();
        let read = self.read_pos.load(Ordering::Acquire);
        let used = write.wrapping_sub(read);
        let cap = (unsafe { &*self.buf.get() }).len();
        let free = cap - used;
        let n = src.len().min(free);
        if n == 0 {
            return 0;
        }

        // SAFETY: [start, start+first) 和 [0, rest) 都落在已分配且未被
        // 消费者读取的区域内，且 start+first <= cap，rest < n <= cap。
        let buf: &mut [f32] = unsafe { &mut *self.buf.get() };
        let start = write & self.mask;
        let first = (cap - start).min(n);
        buf[start..start + first].copy_from_slice(&src[..first]);
        if n > first {
            let rest = n - first;
            buf[..rest].copy_from_slice(&src[first..n]);
        }

        // Release：让消费者 Acquire 读到新的 write_pos 时，也能看到上面写入的数据。
        self.write_pos
            .store(write.wrapping_add(n), Ordering::Release);
        n
    }

    /// 尝试从缓冲取出最多 `dst.len()` 个采样填入 `dst`，返回实际读取的数量。
    ///
    /// 专为音频回调设计：不分配、不加锁、不阻塞。空了就返回 0，
    /// 调用方（回调）自行补静音。
    pub fn pop(&self, dst: &mut [f32]) -> usize {
        let read = self.read_idx();
        let write = self.write_pos.load(Ordering::Acquire);
        let used = write.wrapping_sub(read);
        let n = dst.len().min(used);
        if n == 0 {
            return 0;
        }

        // SAFETY: [start, start+first) 和 [0, rest) 都落在已发布且未被
        // 生产者覆写的区域内。
        let buf: &[f32] = unsafe { &*self.buf.get() };
        let cap = buf.len();
        let start = read & self.mask;
        let first = (cap - start).min(n);
        dst[..first].copy_from_slice(&buf[start..start + first]);
        if n > first {
            let rest = n - first;
            dst[first..n].copy_from_slice(&buf[..rest]);
        }

        self.read_pos
            .store(read.wrapping_add(n), Ordering::Release);
        n
    }

    /// 丢弃缓冲中所有未读采样（用于 Seek 后清空陈旧数据）。
    ///
    /// 只应由消费者或控制线程在回调不会并发 pop 时调用（Seek 时回调可能
    /// 短暂读到空缓冲并补静音，这是可接受的）。
    pub fn clear(&self) {
        // 把读指针推到当前写指针位置，等效于丢弃全部未读数据。
        let write = self.write_pos.load(Ordering::Acquire);
        self.read_pos.store(write, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_roundtrip() {
        let rb = RingBuffer::with_capacity(8);
        let data = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(rb.push(&data), 5);

        let mut out = [0f32; 5];
        assert_eq!(rb.pop(&mut out), 5);
        assert_eq!(out, data);
        assert!(rb.is_empty());
    }

    #[test]
    fn overflow_is_safe() {
        let rb = RingBuffer::with_capacity(4);
        let data = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        // 容量 4，只能写 4 个
        assert_eq!(rb.push(&data), 4);
        assert_eq!(rb.free_space(), 0);

        let mut out = [0f32; 6];
        assert_eq!(rb.pop(&mut out), 4);
        assert_eq!(&out[..4], &[1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn wraparound() {
        let rb = RingBuffer::with_capacity(4);
        // 推入再取出几次，迫使读写指针越过容量边界
        for i in 0..20u32 {
            let v = i as f32;
            assert_eq!(rb.push(&[v]), 1);
            let mut out = [0f32; 1];
            assert_eq!(rb.pop(&mut out), 1);
            assert_eq!(out[0], v);
        }
    }

    #[test]
    fn clear_drops_pending() {
        let rb = RingBuffer::with_capacity(8);
        rb.push(&[1.0, 2.0, 3.0]);
        rb.clear();
        let mut out = [0f32; 3];
        assert_eq!(rb.pop(&mut out), 0);
    }
}
