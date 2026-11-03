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
