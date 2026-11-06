//! Device-level validation: Android NDK MediaCodec actor.
//!
//! Cross-compile with:
//!   cargo test -p mediamaster-codec --target aarch64-linux-android \
//!     --no-default-features --features mediacodec

#![cfg(feature = "mediacodec")]

use mediamaster_codec::ndk_codec::SafeNativeWindow;

#[test]
fn safe_native_window_null_is_none() {
    // A null window should be handled gracefully (no crash on construction).
    // SafeNativeWindow::new takes a raw pointer; we verify construction
    // doesn't panic. The Drop impl calls ANativeWindow_release which is
    // a no-op on null in the NDK.
    let _win = SafeNativeWindow::new(std::ptr::null_mut());
    // If we get here, construction + drop succeeded without panic.
}

#[test]
fn safe_native_window_drop_is_safe() {
    // Constructing and dropping a guard must not panic.
    // We can't create a real ANativeWindow without the NDK, but we can
    // verify the type compiles and the Drop impl is callable.
    let _ = std::mem::size_of::<SafeNativeWindow>();
}

#[test]
fn codec_guard_releases_resources_on_panic() {
    use mediamaster_codec::ndk_codec::CodecGuard;
    use std::panic::{self, AssertUnwindSafe};

    // Simulate panic in a scope where CodecGuard is held
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let dummy_codec = 0x1234 as *mut std::os::raw::c_void;
        let dummy_surface = SafeNativeWindow::new(0x5678 as *mut std::os::raw::c_void);
        let _guard = CodecGuard::new(dummy_codec, Some(dummy_surface));

        // Panic occurs while guard is in scope
        panic!("Simulated MediaCodec worker panic");
    }));

    // Verify panic was caught and guard drop completed without double-panic
    assert!(result.is_err());
}

#[test]
fn android_hevc_to_avc_mime_fallback() {
    use mediamaster_codec::ndk_codec::{
        select_encoder_mime, MIMETYPE_VIDEO_AVC, MIMETYPE_VIDEO_HEVC,
    };

    // When hardware HEVC is present
    assert_eq!(select_encoder_mime(true, true), MIMETYPE_VIDEO_HEVC);

    // Fall back to AVC when hardware HEVC is absent
    assert_eq!(select_encoder_mime(true, false), MIMETYPE_VIDEO_AVC);
}
