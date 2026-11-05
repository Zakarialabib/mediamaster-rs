//! Bounded SPSC ring buffer for `FramePacket` with configurable backpressure.
//!
//! Reuses the same `ringbuf` crate that `mediamaster-audio` uses for its lock-free mixing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ringbuf::ring_buffer::RbBase;
use ringbuf::{Consumer, HeapRb, Producer, Rb};

use crate::packet::FramePacket;

/// Backpressure policy when the ring buffer reaches capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverflowPolicy {
    /// Discard the incoming frame and preserve the buffered frames.
    #[default]
    DropNewest,
    /// Pop and discard the oldest frame in the ring to make room for the new frame.
    OverwriteOldest,
}

enum RingProducerInner {
    /// Pure lock-free SPSC producer when dropping newest on overflow.
    LockFree(Producer<FramePacket, Arc<HeapRb<FramePacket>>>),
    /// Mutex-synchronized access to HeapRb when overwriting oldest, ensuring
    /// single-writer / single-reader safety without violating SPSC invariants or causing races.
    Overwrite {
        rb: Arc<Mutex<HeapRb<FramePacket>>>,
        capacity: usize,
    },
}

enum RingConsumerInner {
    /// Pure lock-free SPSC consumer when dropping newest on overflow.
    LockFree(Consumer<FramePacket, Arc<HeapRb<FramePacket>>>),
    /// Mutex-synchronized access to HeapRb when overwriting oldest.
    Overwrite { rb: Arc<Mutex<HeapRb<FramePacket>>> },
}

/// Producer half of the `FrameRing`.
pub struct FrameRingProducer {
    inner: RingProducerInner,
    policy: OverflowPolicy,
    dropped_count: Arc<AtomicU64>,
}

/// Consumer half of the `FrameRing`.
pub struct FrameRingConsumer {
    inner: RingConsumerInner,
    dropped_count: Arc<AtomicU64>,
}

impl FrameRingProducer {
    /// Push a frame packet according to the configured `OverflowPolicy`.
    ///
    /// Returns `Ok(())` if the frame was stored, or `Err(frame)` if it was dropped
    /// (under `DropNewest`). Under `OverwriteOldest`, if an old frame was dropped,
    /// the drop counter is incremented and `Ok(())` is returned with the new frame stored.
    pub fn push(&mut self, packet: FramePacket) -> Result<(), FramePacket> {
        match &mut self.inner {
            RingProducerInner::LockFree(producer) => {
                if producer.is_full() {
                    self.dropped_count.fetch_add(1, Ordering::Relaxed);
                    Err(packet)
                } else {
                    let _ = producer.push(packet);
                    Ok(())
                }
            }
            RingProducerInner::Overwrite { rb, capacity } => {
                let mut ring = rb.lock().unwrap();
                if ring.is_full() || ring.len() >= *capacity {
                    self.dropped_count.fetch_add(1, Ordering::Relaxed);
                    let _ = ring.pop();
                }
                let _ = ring.push(packet);
                Ok(())
            }
        }
    }

    /// Number of frames dropped due to buffer overflow.
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    /// The configured overflow policy.
    pub fn policy(&self) -> OverflowPolicy {
        self.policy
    }

    /// Remaining slots available before buffer is full.
    pub fn free_len(&self) -> usize {
        match &self.inner {
            RingProducerInner::LockFree(producer) => producer.free_len(),
            RingProducerInner::Overwrite { rb, capacity } => {
                let ring = rb.lock().unwrap();
                capacity.saturating_sub(ring.len())
            }
        }
    }

    /// Whether the ring buffer is full.
    pub fn is_full(&self) -> bool {
        match &self.inner {
            RingProducerInner::LockFree(producer) => producer.is_full(),
            RingProducerInner::Overwrite { rb, capacity } => {
                let ring = rb.lock().unwrap();
                ring.len() >= *capacity
            }
        }
    }
}

impl FrameRingConsumer {
    /// Pop the next available frame packet, or `None` if the ring is empty.
    pub fn pop(&mut self) -> Option<FramePacket> {
        match &mut self.inner {
            RingConsumerInner::LockFree(consumer) => consumer.pop(),
            RingConsumerInner::Overwrite { rb } => {
                let mut ring = rb.lock().unwrap();
                ring.pop()
            }
        }
    }

    /// Number of frames dropped due to buffer overflow on this ring.
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    /// Check if the ring is empty.
    pub fn is_empty(&self) -> bool {
        match &self.inner {
            RingConsumerInner::LockFree(consumer) => consumer.is_empty(),
            RingConsumerInner::Overwrite { rb } => rb.lock().unwrap().is_empty(),
        }
    }

    /// Number of frames currently queued in the ring.
    pub fn len(&self) -> usize {
        match &self.inner {
            RingConsumerInner::LockFree(consumer) => consumer.len(),
            RingConsumerInner::Overwrite { rb } => rb.lock().unwrap().len(),
        }
    }
}

/// A bounded SPSC ring buffer for `FramePacket`.
pub struct FrameRing;

impl FrameRing {
    /// Create a new bounded SPSC frame ring with the specified capacity and overflow policy.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(capacity: usize, policy: OverflowPolicy) -> (FrameRingProducer, FrameRingConsumer) {
        let dropped_count = Arc::new(AtomicU64::new(0));

        match policy {
            OverflowPolicy::DropNewest => {
                let rb = Arc::new(HeapRb::<FramePacket>::new(capacity));
                let producer = unsafe { Producer::new(Arc::clone(&rb)) };
                let consumer = unsafe { Consumer::new(Arc::clone(&rb)) };

                (
                    FrameRingProducer {
                        inner: RingProducerInner::LockFree(producer),
                        policy,
                        dropped_count: Arc::clone(&dropped_count),
                    },
                    FrameRingConsumer {
                        inner: RingConsumerInner::LockFree(consumer),
                        dropped_count,
                    },
                )
            }
            OverflowPolicy::OverwriteOldest => {
                let rb = Arc::new(Mutex::new(HeapRb::<FramePacket>::new(capacity)));
                (
                    FrameRingProducer {
                        inner: RingProducerInner::Overwrite {
                            rb: Arc::clone(&rb),
                            capacity,
                        },
                        policy,
                        dropped_count: Arc::clone(&dropped_count),
                    },
                    FrameRingConsumer {
                        inner: RingConsumerInner::Overwrite { rb },
                        dropped_count,
                    },
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::caps::{Codec, PixelFormat};
    use mediamaster_core::frame::FrameFlags;

    fn make_packet(pts: i64) -> FramePacket {
        FramePacket::new(
            Arc::from([1, 2, 3].as_slice()),
            pts,
            None,
            Codec::H264,
            PixelFormat::Nv12,
            64,
            64,
            64,
            FrameFlags::empty(),
        )
    }

    #[test]
    fn test_ring_drop_newest() {
        let (mut tx, mut rx) = FrameRing::new(2, OverflowPolicy::DropNewest);
        assert!(tx.push(make_packet(1)).is_ok());
        assert!(tx.push(make_packet(2)).is_ok());
        // 3rd push should fail under DropNewest
        let dropped = tx.push(make_packet(3));
        assert!(dropped.is_err());
        assert_eq!(tx.dropped_count(), 1);
        assert_eq!(rx.dropped_count(), 1);

        assert_eq!(rx.pop().unwrap().pts_us, 1);
        assert_eq!(rx.pop().unwrap().pts_us, 2);
        assert!(rx.pop().is_none());
    }

    #[test]
    fn test_ring_overwrite_oldest() {
        let (mut tx, mut rx) = FrameRing::new(2, OverflowPolicy::OverwriteOldest);
        assert!(tx.push(make_packet(1)).is_ok());
        assert!(tx.push(make_packet(2)).is_ok());
        // 3rd push should overwrite oldest (1)
        assert!(tx.push(make_packet(3)).is_ok());
        assert_eq!(tx.dropped_count(), 1);

        assert_eq!(rx.pop().unwrap().pts_us, 2);
        assert_eq!(rx.pop().unwrap().pts_us, 3);
        assert!(rx.pop().is_none());
    }
}
