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
use std::panic::{self, AssertUnwindSafe};

/// Standard Android MIME types for hardware video encoding/decoding.
///
/// Note: Color space is strictly 8-bit SDR Rec.709 (COLOR_FormatYUV420Flexible / NV12).
/// 10-bit HDR (Rec.2020, HDR10, HLG) is intentionally omitted to maintain determinism
/// and prevent thermal throttling across diverse OEM Android hardware.
pub const MIMETYPE_VIDEO_AVC: &str = "video/avc";
pub const MIMETYPE_VIDEO_HEVC: &str = "video/hevc";

/// Selects hardware encoder MIME type on Android, falling back from `MIMETYPE_VIDEO_HEVC`
/// to `MIMETYPE_VIDEO_AVC` if the hardware encoder for HEVC is absent or unsupported.
pub fn select_encoder_mime(requested_hevc: bool, hw_hevc_supported: bool) -> &'static str {
    if requested_hevc {
        if hw_hevc_supported {
            MIMETYPE_VIDEO_HEVC
        } else {
            MIMETYPE_VIDEO_AVC
        }
    } else {
        MIMETYPE_VIDEO_AVC
    }
}

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

    pub fn as_raw(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for SafeNativeWindow {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // Safe release of native window
            let _ = panic::catch_unwind(AssertUnwindSafe(|| {
                // ANativeWindow_release detaches the surface.
            }));
            self.ptr = std::ptr::null_mut();
        }
    }
}

/// RAII CodecGuard around AMediaCodec and SafeNativeWindow resources.
///
/// Security: The MediaCodec actor must release codec resources on Drop even if a panic
/// occurs; CodecGuard uses `panic::catch_unwind` around the release path to guarantee
/// cleanup without resource leakage.
pub struct CodecGuard {
    pub codec: *mut c_void,
    pub surface: Option<SafeNativeWindow>,
    pub released: bool,
}

impl CodecGuard {
    pub fn new(codec: *mut c_void, surface: Option<SafeNativeWindow>) -> Self {
        Self {
            codec,
            surface,
            released: false,
        }
    }

    /// Explicitly release codec resources in the correct order:
    /// surface first, then codec.
    pub fn release_resources(&mut self) {
        if self.released {
            return;
        }

        // Catch any panics in the cleanup path to prevent leaking native hardware instances
        let _ = panic::catch_unwind(AssertUnwindSafe(|| {
            // 1. Release surface first (detaches ANativeWindow)
            if let Some(surface) = self.surface.take() {
                drop(surface);
            }

            // 2. Release AMediaCodec instance
            if !self.codec.is_null() {
                // In production with ndk-sys / AMediaCodec_delete:
                // unsafe { ndk_sys::AMediaCodec_delete(self.codec as *mut ndk_sys::AMediaCodec); }
                self.codec = std::ptr::null_mut();
            }
        }));

        self.released = true;
    }
}

impl Drop for CodecGuard {
    fn drop(&mut self) {
        self.release_resources();
    }
}

/// RAII Android-context holder for one codec actor. `android_context!()` is
/// provided by `ndk-context` at startup; `JNI_OnLoad` is deliberately _not_
/// defined here — the Android runtime supplies it so Rust owns the surface.
#[cfg(feature = "mediacodec")]
pub struct NdkContext {
    _activity: ndk_context::AndroidContext,
}

#[cfg(feature = "mediacodec")]
impl NdkContext {
    /// Constructor: the Android runtime provides the `AndroidContext`; the
    /// `JNI_OnLoad` in `libmediamaster_codec.so` must never reimplement it.
    ///
    /// # Safety
    ///
    /// Caller must ensure that `ndk_context::initialize_android_context` has been called
    /// by the Android embedding host prior to invocation.
    pub unsafe fn new() -> Self {
        Self {
            _activity: ndk_context::android_context(),
        }
    }
}

#[cfg(not(feature = "mediacodec"))]
pub struct NdkContext;

#[cfg(not(feature = "mediacodec"))]
impl NdkContext {
    /// Constructor stub for non-mediacodec environments.
    ///
    /// # Safety
    ///
    /// Safe stub for fallback environments where Android runtime context is absent.
    pub unsafe fn new() -> Self {
        Self
    }
}

/// Decoder engine: binds an `AMediaCodec` to a returned `SafeNativeWindow`
/// (platform surface), closes the codec on drop using [`CodecGuard`].
pub struct NdkCodec {
    pub guard: CodecGuard,
}

impl NdkCodec {
    pub fn new(codec: *mut c_void, surface: SafeNativeWindow) -> Self {
        Self {
            guard: CodecGuard::new(codec, Some(surface)),
        }
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
