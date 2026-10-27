#![allow(unused_imports)]

//! Cross-crate integration tests for the mediamaster-rs workspace.
//!
//! These tests exercise the adapter pattern across domain crates — without
//! tying the workspace to any specific upstream application (a Tauri desktop
//! player, an Android phone app, etc.). They operate purely on the shared
//! trait vocabulary (`mediamaster_core`) and the domain crates.
//!
//! ## Runner
//!
//! ```sh
//! cargo test --package mediamaster-integration-tests --all-features
//! ```

use mediamaster_audio::{MediaSourceAdapter, Mixer};
use mediamaster_core::traits::SensorReading;
use mediamaster_core::{MediamasterError, RoomState};
use mediamaster_rf::{MagnitudeDetector, SignalDetector};
use mediamaster_sensors::fusion::FusionEngine;

#[test]
fn detector_emits_signals_from_spectrum() {
    let detector = MagnitudeDetector::new(8, 0.3);
    let spectrum = vec![0.05, 0.85, 0.20, 0.90, 0.10, 0.05, 0.40, 0.15];
    let mut out = Vec::new();
    let n = detector.detect(&spectrum, &mut out);
    assert_eq!(n, 3, "expected three peaks above 0.3 threshold");
    assert!(out.iter().all(|s| s.confidence >= 0.3));
}

#[test]
fn sensor_fusion_respects_priority() {
    let mut engine = FusionEngine::new();
    engine.update("pir", SensorReading::MotionDetected);
    engine.update("light", SensorReading::Light(10.0));
    assert_eq!(engine.current_state(), RoomState::PersonPresent);

    engine.update("pir", SensorReading::MotionDetected);
    engine.update("light", SensorReading::Light(80.0));
    assert_eq!(engine.current_state(), RoomState::Occupied);
}

#[test]
fn audio_mixer_sums_sources_and_applies_ducking() {
    let mut mixer = Mixer::new();
    mixer.add_source(Box::new(MediaSourceAdapter::dummy("src-a")));
    mixer.add_source(Box::new(MediaSourceAdapter::dummy("src-b")));
    mixer.set_ducking(0.5);
    let out = mixer.mix();
    assert!(!out.is_empty());
    assert!(out.iter().all(|s| (*s - 0.0).abs() < 1e-6));
}

#[test]
fn event_bus_publishes_and_subscribers_receive() {
    let bus = mediamaster_core::EventBus::new(16);
    let mut rx = bus.subscribe();
    assert!(
        bus.publish(mediamaster_core::HardwareEvent::SensorContextChange {
            state: RoomState::Empty,
        })
    );
    if let mediamaster_core::HardwareEvent::SensorContextChange { state } = rx.try_recv().unwrap() {
        assert_eq!(state, RoomState::Empty);
    } else {
        panic!("unexpected event");
    }
}

#[test]
fn error_chain_preserves_context() {
    let err = MediamasterError::Other("sdr crashed".into()).context("rf::pipeline::capture");
    let msg = err.to_string();
    assert!(msg.contains("rf::pipeline::capture"));
    assert!(msg.contains("sdr crashed"));
}
