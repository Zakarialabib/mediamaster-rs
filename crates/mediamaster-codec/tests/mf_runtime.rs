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
