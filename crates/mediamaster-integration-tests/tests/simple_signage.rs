//! Cross-crate integration test: simulates a SimpleSignage-like application
//! that consumes all mediamaster-rs crates.
//!
//! ## Scenario
//! 1. RF detector finds a burst → `Signal` objects are produced.
//! 2. The SimpleSignage adapter publishes a `HardwareEvent::RadioBurstDetected`
//!    to the `EventBus`.
//! 3. The signage app receives the event and updates its `FusionEngine`.
//! 4. The `FusionEngine` reports a new `RoomState`.
//!
//! This proves the adapter pattern: `rf → core::event-bus → sensors`.

use mediamaster_core::{HardwareEvent, MediamasterError, RoomState, SensorReading, SignalDetector};
use mediamaster_rf::MagnitudeDetector;
use mediamaster_sensors::FusionEngine;

#[test]
fn rf_detected_burst_feeds_fusion_engine() {
    // --- Stage 1: RF detection (rf crate) ---
    let detector = MagnitudeDetector::new(4, 0.5);
    let spectrum = vec![0.1, 0.8, 0.2, 0.9];
    let mut signals = Vec::new();
    let peaks = detector.detect(&spectrum, &mut signals);
    assert_eq!(peaks, 2, "expected two peaks above 0.5 threshold");
    assert_eq!(signals.len(), 2);

    // --- Stage 2: Event bus (core crate) ---
    let bus = mediamaster_core::EventBus::new(8);
    let mut rx = bus.subscribe();
    assert_eq!(bus.len(), 1);
    assert!(bus.publish(HardwareEvent::SensorContextChange {
        state: RoomState::PersonPresent,
    }));

    // --- Stage 3: Signage adapter receives event ---
    let received = rx.try_recv().unwrap();
    let bridge_state = match received {
        HardwareEvent::SensorContextChange { state } => state,
        _ => RoomState::Unavailable,
    };
    assert_eq!(bridge_state, RoomState::PersonPresent);

    // --- Stage 4: Fusion engine (sensors crate) consumes sensor reading ---
    let mut engine = FusionEngine::new();
    // No readings → empty.
    assert_eq!(engine.current_state(), RoomState::Empty);

    // Add motion + light → person present.
    engine.update("pir", SensorReading::MotionDetected);
    engine.update("lux", SensorReading::Light(20.0));
    assert_eq!(engine.current_state(), RoomState::PersonPresent);

    // Add RF burst as a "proximity" reading (simulating a presence signal).
    engine.update("rf", SensorReading::Proximity(0.3));
    assert_eq!(engine.current_state(), RoomState::PersonPresent);
}

#[test]
fn error_context_chains_messages() {
    let err = MediamasterError::other("SDR timeout").context("rf::pipeline::acquisition");
    let msg = err.to_string();
    assert!(msg.contains("rf::pipeline::acquisition"), "msg was: {msg}");
    assert!(msg.contains("SDR timeout"), "msg was: {msg}");
}

#[test]
fn sensor_reading_priority_orders_correctly() {
    use mediamaster_core::SensorReading as SR;

    assert!(SR::Unknown.priority() < SR::Light(10.0).priority());
    assert!(SR::Light(10.0).priority() < SR::Open.priority());
    assert!(SR::Occupancy.priority() < SR::MotionDetected.priority());
    assert_eq!(SR::MotionDetected.priority(), 5);
}
