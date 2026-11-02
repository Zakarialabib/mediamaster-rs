//! `EngineSelector` — choose the right transcoding backend per device.
//!
//! The engine crate is the coordination layer: it tells the rest of the system
//! *what* can be done (transcode plan) and *how* (which backend).  `EngineSelector`
//! answers the "how" — pick the right encoder/decoder implementation for the
//! target device, preferring hardware acceleration when the device's
//! `CapsMatrix` says it is available.
//!
//! Backends:
//! - `Engine::Software` — portable software codec (used when the device does not
//!   advertise hardware, or in the default build).
//! - `Engine::MediaFoundation` — Windows `windows 0.62` COM surface (NVENC / NVDEC /
//!   QSV / AMF).  Feature-gated `mf`; device-validation-pending.
//! - `Engine::AndroidNdk` — Android `AMediaCodec` via `ndk-context` / `ndk`.

use super::Codec;

/// The concrete encoder/decoder backend a transcode plan pulls.
pub enum Engine {
    Software,
    MediaFoundation,
    AndroidNdk,
}

impl Engine {
    pub fn hardware_type(&self) -> Option<&'static str> {
        match self {
            Engine::Software => None,
            Engine::MediaFoundation => Some("windows (Media Foundation)"),
            Engine::AndroidNdk => Some("android (AMediaCodec)"),
        }
    }
}

/// Trait for picking an encoder/decoder backend for a given device capability.
pub trait EngineSelector {
    /// Resolve the encoder to use for `codec` on this device.
    fn select_encoder(&self, caps: &mediamaster_core::CapsMatrix, codec: Codec) -> Engine;

    /// Resolve the decoder to use for `codec` on this device.
    fn select_decoder(&self, caps: &mediamaster_core::CapsMatrix, codec: Codec) -> Engine;

    /// Hardware-acceleration status of this selector (true if any hardware
    /// backend is available).
    fn hardware_available(&self, caps: &mediamaster_core::CapsMatrix) -> bool {
        !caps.codecs.is_empty()
    }
}

/// Portable, feature-flag-free implementation of `EngineSelector`.  It only
/// consults the device's `CapsMatrix` (via `mediamaster_core::CapsMatrix`) and
/// returns `Software` when no hardware backend is advertised.  This is the
/// default and publishable path.
pub struct SoftwareCapsDiscovery;

impl Default for SoftwareCapsDiscovery {
    fn default() -> Self {
        Self
    }
}

impl SoftwareCapsDiscovery {
    pub fn new() -> Self {
        Self
    }
}

impl EngineSelector for SoftwareCapsDiscovery {
    fn select_encoder(&self, caps: &mediamaster_core::CapsMatrix, codec: Codec) -> Engine {
        if self.hardware_available(caps) && self.has_encoder(caps, codec) {
            Engine::MediaFoundation
        } else {
            Engine::Software
        }
    }

    fn select_decoder(&self, caps: &mediamaster_core::CapsMatrix, codec: Codec) -> Engine {
        if self.hardware_available(caps) && self.has_decoder(caps, codec) {
            Engine::MediaFoundation
        } else {
            Engine::Software
        }
    }
}

impl SoftwareCapsDiscovery {
    fn has_encoder(&self, caps: &mediamaster_core::CapsMatrix, codec: Codec) -> bool {
        caps.codecs.iter().any(|c| self.matches(codec, &c.codec))
    }

    fn has_decoder(&self, caps: &mediamaster_core::CapsMatrix, codec: Codec) -> bool {
        caps.codecs.iter().any(|c| self.matches(codec, &c.codec))
    }

    fn matches(&self, codec: Codec, caps_codec: &mediamaster_core::Codec) -> bool {
        matches!(
            (codec, caps_codec),
            (Codec::H264, mediamaster_core::Codec::H264)
                | (Codec::Hevc, mediamaster_core::Codec::Hevc)
                | (Codec::Av1, mediamaster_core::Codec::Av1)
        )
    }
}
