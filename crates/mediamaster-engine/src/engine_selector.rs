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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Resolve encoder and degradation tier considering link throughput and slot availability.
    fn select_encoder_with_tier(
        &self,
        caps: &mediamaster_core::CapsMatrix,
        codec: Codec,
        link_throughput: Option<u64>,
        slots: &mut mediamaster_codec::CodecSlotManager,
    ) -> (Engine, mediamaster_core::DegradationTier) {
        let engine = self.select_encoder(caps, codec);
        let tier = self.pick_tier(engine, link_throughput, slots);
        (engine, tier)
    }

    /// Select degradation tier based on engine, link throughput, and slot budget.
    fn pick_tier(
        &self,
        engine: Engine,
        link_throughput: Option<u64>,
        slots: &mut mediamaster_codec::CodecSlotManager,
    ) -> mediamaster_core::DegradationTier {
        // Pacing threshold check: if link throughput is constrained, downgrade tier immediately
        // e.g. < 500 kbps forces poster fallback, < 1.5 Mbps forces shared texture mux or byte buffer
        let max_allowed_tier = match link_throughput {
            Some(tp) if tp < 500_000 => mediamaster_core::DegradationTier::Tier4PosterFallback,
            Some(tp) if tp < 1_500_000 => mediamaster_core::DegradationTier::Tier3SharedTextureMux,
            Some(tp) if tp < 3_000_000 => mediamaster_core::DegradationTier::Tier2HwByteBuffer,
            _ => mediamaster_core::DegradationTier::Tier1HwSurface,
        };

        if matches!(engine, Engine::MediaFoundation | Engine::AndroidNdk) {
            let start_tier = std::cmp::max(
                mediamaster_core::DegradationTier::Tier1HwSurface,
                max_allowed_tier,
            );
            mediamaster_codec::degrade_until(start_tier, |t| match t {
                mediamaster_core::DegradationTier::Tier1HwSurface => {
                    max_allowed_tier <= mediamaster_core::DegradationTier::Tier1HwSurface
                        && slots.acquire(mediamaster_codec::SlotKind::Encoder).is_ok()
                }
                mediamaster_core::DegradationTier::Tier2HwByteBuffer => {
                    max_allowed_tier <= mediamaster_core::DegradationTier::Tier2HwByteBuffer
                        && (slots.can_acquire(mediamaster_codec::SlotKind::Encoder)
                            || slots.acquire(mediamaster_codec::SlotKind::Encoder).is_ok())
                }
                mediamaster_core::DegradationTier::Tier3SharedTextureMux => {
                    max_allowed_tier <= mediamaster_core::DegradationTier::Tier3SharedTextureMux
                }
                mediamaster_core::DegradationTier::Tier4PosterFallback => true,
            })
            .unwrap_or(mediamaster_core::DegradationTier::Tier4PosterFallback)
        } else {
            let start_tier = std::cmp::max(
                mediamaster_core::DegradationTier::Tier2HwByteBuffer,
                max_allowed_tier,
            );
            mediamaster_codec::degrade_until(start_tier, |t| t >= max_allowed_tier)
                .unwrap_or(mediamaster_core::DegradationTier::Tier4PosterFallback)
        }
    }

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
