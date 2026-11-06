//! Device-level validation: Windows Media Foundation actor.
//!
//! Run with: cargo test -p mediamaster-codec --no-default-features --features mf

#![cfg(feature = "mf")]

use mediamaster_codec::mf_actor::spawn_actor;
use mediamaster_core::events::{EventPublisher, HardwareEvent};

struct TestPublisher;
impl EventPublisher for TestPublisher {
    fn publish(&self, _event: HardwareEvent) {}
}

#[test]
fn mf_actor_spawns_without_panic() {
    let (handle, _token, actor_handle) = spawn_actor(
        |_token, guard, session, cmd| {
            // Guard is live; no COM surfaces yet.
            assert!(guard.source_reader.is_none());
            assert!(guard.sink_writer.is_none());
            assert!(guard.transform.is_none());
            assert!(!session.needs_rediscovery);
            match cmd {
                mediamaster_codec::mf_actor::MfCommand::ProcessSample => {
                    Ok(Some(mediamaster_codec::mf_actor::FramePacket {
                        timestamp_hns: 0,
                        duration_hns: 333_333,
                        width: 1920,
                        height: 1080,
                        format: mediamaster_core::caps::PixelFormat::Nv12,
                        data: vec![0u8; 1920 * 1080 * 3 / 2],
                    }))
                }
                _ => Ok(None),
            }
        },
        Some(TestPublisher),
    )
    .expect("spawn_actor should succeed");

    let _ = actor_handle.send(mediamaster_codec::mf_actor::MfCommand::ProcessSample);
    let _ = actor_handle.stop();
    let _ = handle.join();
}

#[test]
fn mf_runtime_guard_drops_surfaces_before_shutdown() {
    use mediamaster_codec::mf_actor::MfRuntimeGuard;
    let mut guard = MfRuntimeGuard {
        source_reader: None,
        sink_writer: None,
        transform: None,
    };
    let dummy = std::ptr::NonNull::new(1 as *mut core::ffi::c_void).unwrap();
    guard.source_reader = Some(dummy);
    guard.sink_writer = Some(dummy);
    guard.transform = Some(dummy);
    // Drop should release all surfaces without panic.
    drop(guard);
}

#[test]
fn mf_gpu_device_manager_validation() {
    use mediamaster_codec::mf_actor::{validate_gpu_device_manager, MfError};

    // Identical GPU device managers are valid
    assert!(validate_gpu_device_manager(Some(123), Some(123)).is_ok());

    // Mismatched GPU device managers are rejected (cross-process leak prevention)
    let res_mismatch = validate_gpu_device_manager(Some(123), Some(456));
    assert!(matches!(
        res_mismatch,
        Err(MfError::GpuDeviceManagerMismatch { .. })
    ));

    // Surface missing GPU device manager when actor requires one is rejected
    let res_missing = validate_gpu_device_manager(Some(123), None);
    assert!(matches!(
        res_missing,
        Err(MfError::GpuDeviceManagerMismatch { .. })
    ));
}

#[test]
fn mf_hevc_midstream_fallback_recovery() {
    use mediamaster_codec::mf_actor::{fallback_hevc_to_h264, MfConfig, MfRuntimeGuard, MfSession};
    use mediamaster_core::caps::Codec;

    let mut guard = MfRuntimeGuard {
        source_reader: None,
        sink_writer: None,
        transform: None,
    };

    let mut session = MfSession {
        current_config: Some(MfConfig {
            codec: Codec::Hevc,
            target_format: "video/hevc".to_string(),
            ..MfConfig::default()
        }),
        active_codec: Some(Codec::Hevc),
        ..MfSession::default()
    };

    // Execute fallback
    let new_cfg = fallback_hevc_to_h264(&mut session, &mut guard).expect("fallback succeeds");

    // Must re-emit Config with H.264
    assert_eq!(new_cfg.codec, Codec::H264);
    assert_eq!(session.active_codec, Some(Codec::H264));
    assert_eq!(session.frames_since_hevc_failure, 0);
    assert!(session.stream_continued);
}
