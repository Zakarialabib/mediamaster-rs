//! Concrete tokio event bus for the mediamaster-rs ecosystem.
//!
//! [`mediamaster-core`] stays `tokio`-free and only defines the runtime-agnostic
//! [`EventPublisher`](mediamaster_core::EventPublisher) trait. This crate owns
//! the concrete `tokio::sync::broadcast` implementation that host runtimes
//! (`mediamaster-agent`, apps, tests) instantiate and inject.

use mediamaster_core::events::{EventPublisher, HardwareEvent};
use tokio::sync::broadcast;

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

impl Default for EventBus {
    fn default() -> Self {
        Self::new(16)
    }
}

impl EventPublisher for EventBus {
    fn publish(&self, event: HardwareEvent) {
        let _ = EventBus::publish(self, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::RoomState;

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
        let bus = EventBus::new(1);
        let published = bus.publish(HardwareEvent::SensorContextChange {
            state: RoomState::Occupied,
        });
        assert!(!published);
    }

    #[test]
    fn event_publisher_trait_dispatches() {
        use mediamaster_core::RoomState as RS;
        let bus = EventBus::new(4);
        let mut rx = bus.subscribe();
        let publisher: &dyn EventPublisher = &bus;
        publisher.publish(HardwareEvent::SensorContextChange {
            state: RS::PersonPresent,
        });
        assert!(matches!(
            rx.try_recv().unwrap(),
            HardwareEvent::SensorContextChange {
                state: RS::PersonPresent
            }
        ));
    }
}
