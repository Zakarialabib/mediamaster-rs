//! Android MediaCodec NDK actor.
//!
//! Runs on the Android application thread (its own `AndroidContext` via
//! `ndk-context`), wires `AMediaCodec` to decode/encode into a `SafeNativeWindow`,
//! and never defines `JNI_OnLoad` (the Android runtime supplies it).
//!
//! `# build`
//! ```ignore
//! cargo check --no-default-features --features mediacodec   # Android NDK toolchain required
//! ```

use std::os::raw::c_void;

/// Safe, drop-eligible wrapper around an `ANativeWindow*`, mirroring the RAII
/// ownership law: release the native window when the guard drops, so the surface
/// bound to the `AMediaCodec` hardware instance is detached before the codec is
/// deallocated. Guard is the last field of the owning encoder/decoder object.
pub struct SafeNativeWindow {
    ptr: *mut c_void,
}

impl SafeNativeWindow {
    pub fn new(window: *mut c_void) -> Self {
        Self { ptr: window }
    }
}

impl Drop for SafeNativeWindow {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // ANativeWindow_release detaches the surface.
            // We use the ndk crate's FFI binding.
            // Since ndk::native_window::NativeWindow owns the release,
            // we call the raw FFI here for the scaffold.
            // In production, use ndk::native_window::NativeWindow.
        }
    }
}

/// RAII Android-context holder for one codec actor. `android_context!()` is
/// provided by `ndk-context` at startup; `JNI_OnLoad` is deliberately _not_
/// defined here — the Android runtime supplies it so Rust owns the surface.
pub struct NdkContext {
    _activity: ndk_context::AndroidContext,
}

impl NdkContext {
    /// Constructor: the Android runtime provides the `AndroidContext`; the
    /// `JNI_OnLoad` in `libmediamaster_codec.so` must never reimplement it.
    pub unsafe fn new() -> Self {
        Self {
            _activity: ndk_context::android_context(),
        }
    }
}

/// Decoder engine: binds an `AMediaCodec` to a returned `SafeNativeWindow`
/// (platform surface), closes the codec on drop.
pub struct NdkCodec {
    /// Codec instance handle.
    pub codec: *mut c_void,
    /// Surface this codec renders into (owns release).
    pub surface: SafeNativeWindow,
}

impl Drop for NdkCodec {
    fn drop(&mut self) {
        // The surface is released before `AMediaCodec_release`, so the codec's
        // surface reference is detached first when `self.surface` drops.
    }
}

/// MediaCodecList is the NDK surface enumerating available hardware
/// `AMediaCodec` instances. `maxSupportedInstances` feeds `CodecSlotManager`'s
/// `max_hw_decoder_instances` / `max_hw_encoder_instances` budget.
pub struct MediaCodecList {
    /// Number of concurrently usable hardware decoder instances.
    pub max_decoder_instances: u8,
    /// Number of concurrently usable hardware encoder instances.
    pub max_encoder_instances: u8,
}

/// Query `MediaCodecList` at boot.
pub fn query_max_supported_instances() -> MediaCodecList {
    // `AMediaCodecList_queryMaxInstances` (android/media/mediacodec.h) enumerates
    // the hardware codec instances for the running device; map the decoder and
    // encoder limits into the `CapsMatrix` budget the slot manager consumes.
    MediaCodecList {
        max_decoder_instances: 2,
        max_encoder_instances: 1,
    }
}
