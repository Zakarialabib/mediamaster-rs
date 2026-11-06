use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use mediamaster_ffi::*;

static LAST_EVENT: Mutex<Option<FfiHardwareEvent>> = Mutex::new(None);
static EVENT_COUNT: AtomicUsize = AtomicUsize::new(0);

extern "C" fn test_event_callback(
    event: *const FfiHardwareEvent,
    _user_data: *mut std::ffi::c_void,
) {
    if !event.is_null() {
        let ev = unsafe { *event };
        let mut guard = LAST_EVENT.lock().unwrap();
        *guard = Some(ev);
        EVENT_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn test_rust_client_mirror_lifecycle_and_events() {
    // 1. Initialize
    assert_eq!(mediamaster_ffi_init(), FfiResult::Ok as i32);

    let ver_ptr = mediamaster_ffi_version();
    assert!(!ver_ptr.is_null());
    let version = unsafe { CStr::from_ptr(ver_ptr).to_str().unwrap() };
    assert_eq!(version, "0.3.0");

    // 2. Register callback
    assert_eq!(
        mediamaster_ffi_set_event_callback(Some(test_event_callback), std::ptr::null_mut()),
        FfiResult::Ok as i32
    );

    // 3. Dispatch an event across FFI
    let ev = FfiHardwareEvent {
        kind: FfiEventKind::SensorContextChange,
        room_state: FfiRoomState::PersonPresent,
        ..Default::default()
    };
    assert_eq!(mediamaster_ffi_dispatch_event(&ev), FfiResult::Ok as i32);

    assert_eq!(EVENT_COUNT.load(Ordering::SeqCst), 1);
    let last = LAST_EVENT.lock().unwrap().take().unwrap();
    assert_eq!(last.kind, FfiEventKind::SensorContextChange);
    assert_eq!(last.room_state, FfiRoomState::PersonPresent);

    // 4. Request mirror session
    let cfg = FfiMirrorConfig {
        session_id: 42,
        width: 1920,
        height: 1080,
        fps: 60,
        enable_audio: 1,
    };
    let mut out_session = 0u64;
    assert_eq!(
        mediamaster_ffi_mirror_requested(&cfg, &mut out_session),
        FfiResult::Ok as i32
    );
    assert_eq!(out_session, 42);

    // 5. Request keyframe
    assert_eq!(
        mediamaster_ffi_mirror_keyframe_requested(42, 1),
        FfiResult::Ok as i32
    );

    // 6. Send input
    let input = FfiInputEvent {
        kind: FfiInputKind::PointerDown,
        x: 100.5,
        y: 200.5,
        button: 1,
        key_code: 0,
    };
    assert_eq!(
        mediamaster_ffi_mirror_input(42, &input),
        FfiResult::Ok as i32
    );

    // 7. Deliver packet to sink
    let sample_data = [0xde, 0xad, 0xbe, 0xef];
    let mut written = 0usize;
    assert_eq!(
        mediamaster_ffi_mirror_sink(
            42,
            1, // is_video
            1, // is_keyframe
            1000000,
            sample_data.as_ptr(),
            sample_data.len(),
            &mut written
        ),
        FfiResult::Ok as i32
    );
    assert_eq!(written, 4);

    // 8. Stop mirror session
    assert_eq!(
        mediamaster_ffi_mirror_stop_requested(42),
        FfiResult::Ok as i32
    );

    // Operations after stop return SessionNotFound (-2)
    assert_eq!(
        mediamaster_ffi_mirror_keyframe_requested(42, 1),
        FfiResult::SessionNotFound as i32
    );

    // 9. Shutdown
    assert_eq!(mediamaster_ffi_shutdown(), FfiResult::Ok as i32);
}
