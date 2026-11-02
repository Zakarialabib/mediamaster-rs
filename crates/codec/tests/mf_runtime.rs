//! Device-level validation: Windows Media Foundation actor.
//!
//! Run with: cargo test -p mediamaster-codec --no-default-features --features mf

#![cfg(feature = "mf")]

use mediamaster_codec::mf_actor::spawn_actor;

#[test]
fn mf_actor_spawns_without_panic() {
    let (handle, _token) = spawn_actor(|_token, guard| {
        // Guard is live; no COM surfaces yet.
        assert!(guard.source_reader.is_none());
        assert!(guard.sink_writer.is_none());
        assert!(guard.transform.is_none());
        Ok(())
    })
    .expect("spawn_actor should succeed");

    // The actor thread blocks on rx.recv(); detach it.
    // We can't send Stop (channel is internal), so just leak the handle.
    std::mem::forget(handle);
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
