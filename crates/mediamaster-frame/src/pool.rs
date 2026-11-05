//! Reusable Arc buffer pool to eliminate per-frame allocations.
//!
//! Allocating multi-megabyte buffers on each decoded video frame induces severe allocator
//! fragmentation and latency jitter. [`FramePool`] maintains a bounded pool of pre-allocated
//! memory buffers. Checked-out buffers wrapped in [`PooledBuffer`] return themselves to the
//! pool upon dropping, ensuring peak allocations remain bounded by the pool size
//! after initial warmup.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::packet::{FramePacket, FramePayload, PoolReclaim, PooledPayload};
use mediamaster_core::caps::{Codec, PixelFormat};
use mediamaster_core::frame::{FrameFlags, FrameRef};

/// Shared pool state holding reusable memory chunks.
struct PoolInner {
    free_buffers: Mutex<Vec<Vec<u8>>>,
    buffer_capacity: usize,
    max_pool_size: usize,
    total_allocations: AtomicUsize,
}

impl PoolReclaim for PoolInner {
    fn reclaim(&self, mut buf: Vec<u8>) {
        buf.clear();
        let mut free = self.free_buffers.lock().unwrap();
        if free.len() < self.max_pool_size {
            free.push(buf);
        }
    }
}

/// A handle to a buffer checked out from [`FramePool`].
///
/// When dropped, returns the buffer back to the pool unless converted into a [`FramePacket`],
/// in which case the buffer returns to the pool when the last clone of the `FramePacket` drops.
pub struct PooledBuffer {
    buffer: Option<Vec<u8>>,
    inner: Arc<PoolInner>,
}

impl PooledBuffer {
    /// Access the buffer as a slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.buffer.as_deref().unwrap_or(&[])
    }

    /// Access the mutable buffer.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.buffer.as_deref_mut().unwrap_or(&mut [])
    }

    /// Write bytes into this pooled buffer.
    pub fn set_data(&mut self, src: &[u8]) {
        if let Some(buf) = self.buffer.as_mut() {
            buf.clear();
            let to_copy = src.len().min(buf.capacity());
            buf.extend_from_slice(&src[..to_copy]);
        }
    }

    /// Convert this pooled buffer into a `FramePacket` with zero-copy pool recycling.
    ///
    /// When all clones of the resulting `FramePacket` are dropped, the backing vector
    /// is returned directly to the `FramePool`'s free list without deallocation.
    #[allow(clippy::too_many_arguments)]
    pub fn into_frame_packet(
        mut self,
        pts_us: i64,
        dts_us: Option<i64>,
        codec: Codec,
        pixel_format: PixelFormat,
        width: u16,
        height: u16,
        stride: u16,
        flags: FrameFlags,
    ) -> FramePacket {
        let buf = self.buffer.take().unwrap_or_default();
        let payload = FramePayload::Pooled(Arc::new(PooledPayload {
            data: buf,
            reclaimer: Arc::clone(&self.inner) as Arc<dyn PoolReclaim>,
        }));

        FramePacket::with_payload(
            payload,
            pts_us,
            dts_us,
            codec,
            pixel_format,
            width,
            height,
            stride,
            flags,
        )
    }
}

impl core::ops::Deref for PooledBuffer {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl core::ops::DerefMut for PooledBuffer {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let Some(buf) = self.buffer.take() {
            self.inner.reclaim(buf);
        }
    }
}

/// A bounded pool of reusable frame buffers.
#[derive(Clone)]
pub struct FramePool {
    inner: Arc<PoolInner>,
}

impl FramePool {
    /// Create a new `FramePool` with a maximum number of pooled buffers,
    /// each with at least `buffer_capacity` bytes.
    pub fn new(max_pool_size: usize, buffer_capacity: usize) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                free_buffers: Mutex::new(Vec::with_capacity(max_pool_size)),
                buffer_capacity,
                max_pool_size,
                total_allocations: AtomicUsize::new(0),
            }),
        }
    }

    /// Pre-allocate (warm up) all buffers in the pool.
    pub fn warmup(&self) {
        let mut free = self.inner.free_buffers.lock().unwrap();
        while free.len() < self.inner.max_pool_size {
            let buf = Vec::with_capacity(self.inner.buffer_capacity);
            free.push(buf);
            self.inner.total_allocations.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Acquire a buffer from the pool, allocating a new one only if the pool is empty.
    pub fn acquire(&self) -> PooledBuffer {
        let mut free = self.inner.free_buffers.lock().unwrap();
        let buffer = if let Some(buf) = free.pop() {
            buf
        } else {
            self.inner.total_allocations.fetch_add(1, Ordering::Relaxed);
            Vec::with_capacity(self.inner.buffer_capacity)
        };

        PooledBuffer {
            buffer: Some(buffer),
            inner: Arc::clone(&self.inner),
        }
    }

    /// Promote a [`FrameRef`] into a [`FramePacket`] using a recycled buffer.
    ///
    /// Copies `frame_ref.bytes` into a pooled buffer and returns a zero-copy
    /// recyclable `FramePacket`.
    pub fn promote_ref(&self, frame_ref: &FrameRef<'_>) -> FramePacket {
        let mut pooled = self.acquire();
        pooled.set_data(frame_ref.bytes);
        pooled.into_frame_packet(
            frame_ref.pts_us,
            frame_ref.dts_us,
            frame_ref.codec,
            frame_ref.pixel_format,
            frame_ref.width,
            frame_ref.height,
            frame_ref.stride,
            frame_ref.flags,
        )
    }

    /// Total number of heap allocations performed by this pool.
    pub fn total_allocations(&self) -> usize {
        self.inner.total_allocations.load(Ordering::Relaxed)
    }

    /// Number of free buffers currently available in the pool.
    pub fn free_count(&self) -> usize {
        self.inner.free_buffers.lock().unwrap().len()
    }

    /// Configured maximum pool capacity.
    pub fn max_pool_size(&self) -> usize {
        self.inner.max_pool_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_recycling() {
        let pool = FramePool::new(4, 1024);
        assert_eq!(pool.total_allocations(), 0);

        // Warm up pool
        pool.warmup();
        assert_eq!(pool.total_allocations(), 4);
        assert_eq!(pool.free_count(), 4);

        // Acquire 2 buffers
        let b1 = pool.acquire();
        let b2 = pool.acquire();
        assert_eq!(pool.free_count(), 2);
        assert_eq!(pool.total_allocations(), 4);

        // Drop one buffer -> should return to pool
        drop(b1);
        assert_eq!(pool.free_count(), 3);
        assert_eq!(pool.total_allocations(), 4);

        // Acquire again -> no new allocations
        let _b3 = pool.acquire();
        assert_eq!(pool.free_count(), 2);
        assert_eq!(pool.total_allocations(), 4);

        drop(b2);
        drop(_b3);
        assert_eq!(pool.free_count(), 4);
    }
}
