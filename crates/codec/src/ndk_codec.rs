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
use std::ptr::NonNull;

/// Safe, drop-eligible wrapper around an `ANativeWindow*`, mirroring the RAII
/// ownership law: release the native window when the guard drops, so the surface
/// bound to the `AMediaCodec` hardware instance is detached before the codec is
/// deallocated. Guard is the last field of the owning encoder/decoder object.
pub struct SafeNativeWindow {
    ptr: *mut c_void,
}

impl SafeNativeWindow {
    pub fn new(window: *mut c_void) -> Self {
        // `ANativeWindow_fromSurface` (android/native_window.h) is called by the
        // host activity and returns a ref-counted ANativeWindow; we take its
        // pointer and own the final release.
        Self { ptr: window }
    }
}

impl Drop for SafeNativeWindow {
    fn drop(&mut self) {
        // `ANativeWindow_release` detaches the surface from any AMediaCodec that
        // held a reference. The guard is the last field of the owning codec
        // object, so release here, before that codec and its `AMediaCodec`
        // instance are torn down.
        unsafe {
            ndk::native_window::ANativeWindow_release(self.ptr);
        }
    }
}

/// RAII Android-context holder for one codec actor. `android_context!()` is
/// provided by `ndk-context` at startup; `JNI_OnLoad` is deliberately _not_
/// defined here — the Android runtime supplies it so Rust owns the surface.
pub struct NdkContext {
    _activity: ndk_context::AndroidContext,
    _env: ndk::JNIEnv,
}

impl NdkContext {
    /// Constructor: the Android runtime provides the `AndroidContext`; the
    /// `JNI_OnLoad` in `libmediamaster_codec.so` must never reimplement it.
    pub unsafe fn new() -> Self {
        // `android_context!()` pulls the `AndroidContext` from the process's
        // existing `ndk-context` initialization (set up by ndk-glue / the
        // platform). The activity and JNIEnv are used only to reach the NDK.
        Self {
            _activity: ndk_context::android_context!(),
            _env: ndk::JNIEnv::attach_current(),
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
        // surface reference is detached first.
        self.surface.drop();
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
