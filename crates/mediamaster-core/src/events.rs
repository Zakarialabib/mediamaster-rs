//! Event bus + hardware event types.

use crate::types::{BoundingBox, RadioBurst, RoomState};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Every event the ecosystem publishes and consumers subscribe to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HardwareEvent {
    /// High-priority event requested a volume duck on a named audio source.
    AudioDuckRequest {
        source: String,
        ducking: f32,
        until: u64,
    },
    /// A debounced sensor fusion state changed.
    SensorContextChange { state: RoomState },
    /// An RF burst was detected in the spectrum.
    RadioBurstDetected { burst: RadioBurst },
    /// A CCTV camera reported motion.
    CctvMotionDetected {
        camera: String,
        bounding_box: BoundingBox,
    },
}

impl HardwareEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            HardwareEvent::AudioDuckRequest { .. } => "AudioDuckRequest",
            HardwareEvent::SensorContextChange { .. } => "SensorContextChange",
            HardwareEvent::RadioBurstDetected { .. } => "RadioBurstDetected",
            HardwareEvent::CctvMotionDetected { .. } => "CctvMotionDetected",
        }
    }
}

/// A broadcast-style event bus. All crates publish to it; adapters subscribe.
///
/// Internally wraps `tokio::sync::broadcast`. Subscribers receive a
/// `broadcast::Receiver` that can be polled with `recv()` (async) or
/// `try_recv()` (sync). If the channel buffer fills while no subscriber is
/// reading, `publish` returns `false` (event was dropped).
pub struct EventBus {
    sender: broadcast::Sender<HardwareEvent>,
    capacity: usize,
}

impl EventBus {
    /// Create a new bus with the given subscriber capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender, capacity }
    }

    /// Subscribe to the event stream.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<HardwareEvent> {
        self.sender.subscribe()
    }

    /// Publish an event to all current subscribers.
    ///
    /// Returns `false` if there are no subscribers or the channel is full.
    #[must_use]
    pub fn publish(&self, event: HardwareEvent) -> bool {
        self.sender.send(event).is_ok()
    }

    /// Number of live subscribers.
    pub fn len(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Returns `true` when there are no live subscribers.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The capacity of each subscriber's lag buffer.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_subscribers_initially() {
        let bus = EventBus::new(16);
        assert!(bus.is_empty());
        assert_eq!(bus.len(), 0);
        assert_eq!(bus.capacity(), 16);
    }

    #[test]
    fn publish_and_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        assert_eq!(bus.len(), 1);
        assert!(!bus.is_empty());

        let event = HardwareEvent::AudioDuckRequest {
            source: "test".into(),
            ducking: 0.5,
            until: 100,
        };
        assert!(bus.publish(event.clone()));

        let received = rx.try_recv().unwrap();
        assert_eq!(received, event);
        assert_eq!(received.event_name(), "AudioDuckRequest");
    }

    #[test]
    fn publish_to_no_subscribers_returns_false() {
        // With no live subscribers, publish returns false.
        let bus = EventBus::new(1);
        let published = bus.publish(HardwareEvent::AudioDuckRequest {
            source: "noop".into(),
            ducking: 0.0,
            until: 0,
        });
        assert!(!published);
    }
}
