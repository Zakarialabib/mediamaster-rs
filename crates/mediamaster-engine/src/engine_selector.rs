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
//! - `Engine::AndroidNdk` — Android `AMediaCodec` via `ndk-context` / `ndk`.`
//!   Feature-gated `mediacodec`; device-validation-pending.

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
    fn select_encoder(&self, caps: &mediamaster_codec::CapsMatrix, codec: Codec) -> Engine;

    /// Resolve the decoder to use for `codec` on this device.
    fn select_decoder(&self, caps: &mediamaster_codec::CapsMatrix, codec: Codec) -> Engine;

    /// Hardware-acceleration status of this selector (true if any hardware
    /// backend is available).
    fn hardware_available(&self, caps: &mediamaster_codec::CapsMatrix) -> bool {
        !caps.encoder.is_empty() || !caps.decoder.is_empty()
    }
}

/// Portable, feature-flag-free implementation of `EngineSelector`.  It only
/// consults the device's `CapsMatrix` (via `mediamaster_codec::CapsMatrix`) and
/// returns `Software` when no hardware backend is advertised.  This is the
/// default and publishable path.
pub struct SoftwareCapsDiscovery;

impl SoftwareCapsDiscovery {
    pub fn new() -> Self {
        Self
    }
}

impl EngineSelector for SoftwareCapsDiscovery {
    fn select_encoder(&self, caps: &mediamaster_codec::CapsMatrix, codec: Codec) -> Engine {
        if self.hardware_available(caps) && self.has_encoder(caps, codec) {
            Engine::MediaFoundation
        } else {
            Engine::Software
        }
    }

    fn select_decoder(&self, caps: &mediamaster_codec::CapsMatrix, codec: Codec) -> Engine {
        if self.hardware_available(caps) && self.has_decoder(caps, codec) {
            Engine::MediaFoundation
        } else {
            Engine::Software
        }
    }
}

impl SoftwareCapsDiscovery {
    fn has_encoder(&self, caps: &mediamaster_codec::CapsMatrix, codec: Codec) -> bool {
        caps.encoder.iter().any(|c| self.matches(codec, c))
    }

    fn has_decoder(&self, caps: &mediamaster_codec::CapsMatrix, codec: Codec) -> bool {
        caps.decoder.iter().any(|c| self.matches(codec, c))
    }

    fn matches(&self, codec: Codec, caps_codec: &mediamaster_codec::Codec) -> bool {
        match (codec, caps_codec) {
            (Codec::H264, mediamaster_codec::Codec::H264)
            | (Codec::Hevc, mediamaster_codec::Codec::Hevc)
            | (Codec::Av1, mediamaster_codec::Codec::Av1) => true,
            _ => false,
        }
    }
}
