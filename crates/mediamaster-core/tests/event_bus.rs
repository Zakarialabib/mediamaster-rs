//! Integration test: EventBus carries HardwareEvent across the crate boundary.

use mediamaster_core::events::{EventBus, HardwareEvent};

#[test]
fn publish_and_receive_hardware_event() {
    let bus = EventBus::new(16);
    let mut rx = bus.subscribe();

    let event = HardwareEvent::AudioDuckRequest {
        source: "narrator".into(),
        ducking: 0.5,
        until: 100,
    };
    assert!(bus.publish(event));

    let received = rx.try_recv().unwrap();
    assert_eq!(received.event_name(), "AudioDuckRequest");
}
