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
    let bus = mediamaster_bus::EventBus::new(16);
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

#[tokio::test]
async fn test_cctv_motion_to_emergency_asset_and_full_mute_with_audit() {
    use mediamaster_agent::Agent;
    use mediamaster_bridge::AudioBridge;
    use mediamaster_bus::EventBus;
    use mediamaster_core::{HardwareEvent, RoomState, SignageCommand};
    use std::sync::Arc;
    use std::time::Instant;

    let bus = Arc::new(EventBus::new(32));
    let mut rx = bus.subscribe();
    let audio = Arc::new(AudioBridge::new());
    let emergency_blake3 = [0x55; 32];
    let agent =
        Agent::new(bus.clone(), audio.clone()).with_emergency_asset("zone-lobby", emergency_blake3);

    // Run agent in background
    let run_handle = tokio::spawn(async move {
        let _ = agent.run().await;
    });

    // Allow background task to subscribe
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let start = Instant::now();

    // 1. Simulate CCTV Detection event via SensorReading -> FusionEngine -> RoomState
    let mut fusion = FusionEngine::new();
    fusion.update(
        "cctv-entrance",
        SensorReading::CctvDetection {
            confidence: 0.98,
            is_emergency: true,
        },
    );
    let state = fusion.current_state();
    assert_eq!(state, RoomState::EmergencyOverride);

    // Publish CCTV motion detected event (for signature provenance) and state change
    let _ = bus.publish(HardwareEvent::CctvMotionDetected {
        camera: "cctv-entrance".to_string(),
        bounding_box: mediamaster_core::BoundingBox {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        },
    });

    let _ = bus.publish(HardwareEvent::SensorContextChange { state });

    // 2. Panel switches to emergency asset within 1 s
    let mut got_signage_cmd = false;
    let mut got_duck_cmd = false;
    let mut got_audit_entry = false;

    let timeout = std::time::Duration::from_millis(900);
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline && (!got_signage_cmd || !got_duck_cmd || !got_audit_entry) {
        if let Ok(event) = rx.try_recv() {
            match event {
                HardwareEvent::Signage {
                    command: SignageCommand::EmergencyAsset { zone, asset_blake3 },
                } => {
                    assert_eq!(zone, "zone-lobby");
                    assert_eq!(asset_blake3, emergency_blake3);
                    assert!(start.elapsed().as_secs_f64() <= 1.0);
                    got_signage_cmd = true;
                }
                HardwareEvent::AudioDuckRequest { ducking: 0.0, .. } => {
                    got_duck_cmd = true;
                }
                HardwareEvent::AuditLogged { entry } => {
                    // Check all three signatures exist
                    assert!(entry.cctv_detection_sig.contains("cctv-entrance"));
                    assert!(entry.signage_command_sig.contains("zone-lobby"));
                    assert!(entry.audio_ducking_sig.contains("FULL_MUTE"));
                    got_audit_entry = true;
                }
                _ => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(
        got_signage_cmd,
        "Expected SignageCommand::EmergencyAsset within 1s"
    );
    assert!(got_duck_cmd, "Expected AudioDuckRequest to FULL_MUTE");
    assert!(
        got_audit_entry,
        "Expected DomainAuditEntry with all three signatures"
    );
    assert!(audio.mixer().lock().unwrap().ducking() < 1.0);
    run_handle.abort();
}

#[test]
fn test_link_throughput_drop_picks_lower_tier_within_1_rtt() {
    use mediamaster_codec::slot_budget::CodecSlotManager;
    use mediamaster_core::caps::{CapsMatrix, Codec, CodecCaps, DegradationTier, PixelFormat};
    use mediamaster_engine::engine_selector::{EngineSelector, SoftwareCapsDiscovery};
    use mediamaster_engine::transcode_plan::TranscodePlan;
    use std::time::Instant;

    let caps = CapsMatrix {
        codecs: vec![CodecCaps {
            codec: Codec::H264,
            profile: "High".into(),
            level: "4.1".into(),
            max_width: 1920,
            max_height: 1080,
            max_fps: 60,
            pixel_formats: vec![PixelFormat::Nv12],
            hw_accelerated: true,
        }],
        max_hw_decoder_instances: 4,
        max_hw_encoder_instances: 4,
        verified_on_hardware: true,
    };

    let selector = SoftwareCapsDiscovery::new();
    let mut slots = CodecSlotManager::new(&caps);

    // Initial plan with high link throughput (10 Mbps)
    let plan_high =
        TranscodePlan::for_device(1920, 1080, &caps).with_link_throughput(Some(10_000_000));
    let (_, tier_high) = selector.select_encoder_with_tier(
        &caps,
        plan_high.profile.codec,
        plan_high.link_throughput,
        &mut slots,
    );
    assert_eq!(tier_high, DegradationTier::Tier1HwSurface);

    // Measured RTT (e.g., 50ms)
    let rtt = std::time::Duration::from_millis(50);
    let start = Instant::now();

    // Link throughput drops sharply to 400 kbps (e.g. 50%+ drop / network congestion)
    let plan_low = TranscodePlan::for_device(1920, 1080, &caps).with_link_throughput(Some(400_000));
    let (_, tier_low) = selector.select_encoder_with_tier(
        &caps,
        plan_low.profile.codec,
        plan_low.link_throughput,
        &mut slots,
    );

    let elapsed = start.elapsed();
    assert!(
        elapsed < rtt,
        "Tier downgrade decision must complete within 1 RTT (elapsed: {elapsed:?}, RTT: {rtt:?})"
    );
    assert!(
        tier_low > tier_high,
        "Tier must downgrade from Tier1 to lower tier"
    );
    assert_eq!(tier_low, DegradationTier::Tier4PosterFallback);
}

#[test]
fn test_audio_video_sync_drift_under_50_percent_bandwidth_drop() {
    use mediamaster_audio::jitter::PagingJitterBuffer;

    let sample_rate = 48_000;
    // Initial prebuffer/video latency: 50 ms
    let mut jb = PagingJitterBuffer::new(sample_rate, 50);

    // Prime the buffer at nominal network pacing
    let frame_size = 960; // 20ms of 48kHz audio
    for _ in 0..5 {
        jb.push(&vec![0.5f32; frame_size]);
    }
    assert!(jb.is_primed());

    // Video path running at 50 ms latency
    jb.set_video_latency_ms(50.0);
    let ack_nominal = jb.generate_ack();
    assert!(ack_nominal.av_sync_drift_ms.abs() < 50.0);

    // Simulate 50% bandwidth drop:
    // Audio packet arrival rate halves / packets delayed; video adjusts latency to 75 ms
    jb.set_video_latency_ms(75.0);

    // Drain some samples while receiving fewer frames (50% drop simulation)
    for _ in 0..1000 {
        let _ = jb.pop_output();
    }
    jb.push(&vec![0.4f32; frame_size / 2]);

    for _ in 0..1000 {
        let _ = jb.pop_output();
    }

    // Check A/V sync drift: must be <= 80 ms
    let ack_after_drop = jb.generate_ack();
    assert!(
        ack_after_drop.av_sync_drift_ms.abs() <= 80.0,
        "A/V sync drift {:?} ms exceeded 80 ms threshold under 50% drop",
        ack_after_drop.av_sync_drift_ms
    );
}
