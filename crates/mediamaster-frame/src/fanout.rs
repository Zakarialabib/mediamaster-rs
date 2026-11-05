//! Single producer → N subscribers fan-out for `FramePacket`.
//!
//! Each subscriber receives its own ring buffer / cursor. Slow subscribers that overflow
//! their queues drop frames and emit [`HardwareEvent::FrameDrop`] onto the event bus.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use mediamaster_core::events::{EventPublisher, HardwareEvent};

use crate::packet::FramePacket;
use crate::ring::{FrameRing, FrameRingConsumer, FrameRingProducer, OverflowPolicy};

/// Identifier for a fan-out subscriber.
pub type SubscriberId = usize;

/// Internal entry for a registered subscriber.
struct SubscriberSlot {
    id: SubscriberId,
    producer: FrameRingProducer,
    active: Arc<AtomicBool>,
    total_drops: Arc<AtomicU64>,
}

/// A subscriber handle returned by [`FanOut::subscribe`].
pub struct FanOutSubscriber {
    id: SubscriberId,
    consumer: FrameRingConsumer,
    active: Arc<AtomicBool>,
}

impl FanOutSubscriber {
    /// The unique subscriber id.
    pub fn id(&self) -> SubscriberId {
        self.id
    }

    /// Pop the next available frame packet.
    pub fn pop(&mut self) -> Option<FramePacket> {
        self.consumer.pop()
    }

    /// Number of dropped frames for this subscriber.
    pub fn dropped_count(&self) -> u64 {
        self.consumer.dropped_count()
    }

    /// Check if queue is empty.
    pub fn is_empty(&self) -> bool {
        self.consumer.is_empty()
    }

    /// Number of queued frames.
    pub fn len(&self) -> usize {
        self.consumer.len()
    }
}

impl Drop for FanOutSubscriber {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

/// Single producer → N subscriber fan-out distributor.
pub struct FanOut {
    subscribers: Vec<SubscriberSlot>,
    next_id: usize,
    default_capacity: usize,
    default_policy: OverflowPolicy,
    bus: Option<Arc<dyn EventPublisher>>,
}

impl FanOut {
    /// Create a new `FanOut` with default queue capacity per subscriber.
    pub fn new(capacity: usize, policy: OverflowPolicy) -> Self {
        Self {
            subscribers: Vec::new(),
            next_id: 1,
            default_capacity: capacity,
            default_policy: policy,
            bus: None,
        }
    }

    /// Attach an [`EventPublisher`] (such as `EventBus`) to emit `FrameDrop` events.
    pub fn with_bus(mut self, bus: Arc<dyn EventPublisher>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Set or change the attached event bus.
    pub fn set_bus(&mut self, bus: Arc<dyn EventPublisher>) {
        self.bus = Some(bus);
    }

    /// Register a new subscriber with default capacity and policy.
    pub fn subscribe(&mut self) -> FanOutSubscriber {
        self.subscribe_with_config(self.default_capacity, self.default_policy)
    }

    /// Register a new subscriber with custom queue capacity and policy.
    pub fn subscribe_with_config(
        &mut self,
        capacity: usize,
        policy: OverflowPolicy,
    ) -> FanOutSubscriber {
        let id = self.next_id;
        self.next_id += 1;

        let (producer, consumer) = FrameRing::new(capacity, policy);
        let active = Arc::new(AtomicBool::new(true));
        let total_drops = Arc::new(AtomicU64::new(0));

        let slot = SubscriberSlot {
            id,
            producer,
            active: Arc::clone(&active),
            total_drops,
        };
        self.subscribers.push(slot);

        FanOutSubscriber {
            id,
            consumer,
            active,
        }
    }

    /// Broadcast a `FramePacket` to all active subscribers.
    ///
    /// If a subscriber queue overflows, the frame is dropped or oldest evicted according
    /// to that subscriber's policy, and a [`HardwareEvent::FrameDrop`] event is published.
    pub fn broadcast(&mut self, packet: FramePacket) {
        // Retain only still-active subscribers.
        self.subscribers
            .retain(|s| s.active.load(Ordering::Acquire));

        for slot in &mut self.subscribers {
            let drops_before = slot.producer.dropped_count();
            // Clone the packet for this subscriber (zero-copy clone of Arc payload).
            let res = slot.producer.push(packet.clone());
            let drops_after = slot.producer.dropped_count();

            if drops_after > drops_before || res.is_err() {
                let delta = drops_after.saturating_sub(drops_before).max(1);
                slot.total_drops.fetch_add(delta, Ordering::Relaxed);
                if let Some(bus) = &self.bus {
                    bus.publish(HardwareEvent::FrameDrop {
                        subscriber_id: slot.id,
                        count: delta,
                    });
                }
            }
        }
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers
            .iter()
            .filter(|s| s.active.load(Ordering::Acquire))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::caps::{Codec, PixelFormat};
    use mediamaster_core::frame::FrameFlags;
    use std::sync::Mutex;

    struct TestBus {
        events: Mutex<Vec<HardwareEvent>>,
    }

    impl TestBus {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn drops(&self) -> Vec<(usize, u64)> {
            let events = self.events.lock().unwrap();
            events
                .iter()
                .filter_map(|e| match e {
                    HardwareEvent::FrameDrop {
                        subscriber_id,
                        count,
                    } => Some((*subscriber_id, *count)),
                    _ => None,
                })
                .collect()
        }
    }

    impl EventPublisher for TestBus {
        fn publish(&self, event: HardwareEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    fn sample_frame(pts: i64) -> FramePacket {
        FramePacket::new(
            Arc::from(vec![1, 2, 3].into_boxed_slice()),
            pts,
            None,
            Codec::H264,
            PixelFormat::Nv12,
            128,
            128,
            128,
            FrameFlags::empty(),
        )
    }

    #[test]
    fn test_fanout_subscribers_and_drop_event() {
        let bus = Arc::new(TestBus::new());
        let mut fanout = FanOut::new(2, OverflowPolicy::DropNewest).with_bus(bus.clone());

        let mut sub1 = fanout.subscribe();
        let _sub2 = fanout.subscribe();

        fanout.broadcast(sample_frame(10));
        fanout.broadcast(sample_frame(20));

        // Sub1 reads one frame
        assert_eq!(sub1.pop().unwrap().pts_us, 10);
        // Sub2 does not read anything (queue full at 2 frames)

        // 3rd broadcast: fits in sub1, but overflows sub2
        fanout.broadcast(sample_frame(30));

        assert_eq!(sub1.pop().unwrap().pts_us, 20);
        assert_eq!(sub1.pop().unwrap().pts_us, 30);
        assert_eq!(sub1.dropped_count(), 0);

        // Sub2 should have dropped 1 frame
        assert_eq!(_sub2.dropped_count(), 1);
        let drops = bus.drops();
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0], (_sub2.id(), 1));
    }
}
