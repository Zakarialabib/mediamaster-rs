//! Integration test: FusionEngine consumes SensorReading (core) across crate boundary.

use mediamaster_core::traits::SensorReading;
use mediamaster_core::types::RoomState;
use mediamaster_sensors::FusionEngine;

#[test]
fn fusion_priority_motion_trumps_light() {
    let mut engine = FusionEngine::new();
    engine.update("light", SensorReading::Light(10.0));
    assert_eq!(engine.current_state(), RoomState::LowLight);

    engine.update("pir", SensorReading::MotionDetected);
    assert_eq!(engine.current_state(), RoomState::Occupied);
}

#[test]
fn fusion_no_readings_is_empty() {
    let engine = FusionEngine::new();
    assert_eq!(engine.current_state(), RoomState::Empty);
}

#[test]
fn fusion_proximity_below_threshold() {
    let mut engine = FusionEngine::new();
    engine.update("proximity", SensorReading::Proximity(5.0));
    assert_eq!(engine.current_state(), RoomState::PersonPresent);
}
