//! Platform hooks that enumerate hardware codecs into a [`mediamaster_core::CapsMatrix`].
//!
//! The OS (Windows Media Foundation, Android MediaCodec) is the source of truth
//! for what hardware a host actually has. This module defines the seam those
//! platform backends plug into; [`SoftwareCapsDiscovery`] is the fallback when
//! no platform backend is compiled in.

use mediamaster_core::{CapsMatrix, Codec, CodecCaps, PixelFormat};

/// Enumerates what a host can actually decode/encode into a [`mediamaster_core::CapsMatrix`].
pub trait CapsDiscovery {
    /// Probe the host and return its capability matrix.
    fn discover(&self) -> CapsMatrix;
}

/// Software-only discovery: advertises the software-decodable codec set with no
/// hardware acceleration and an unknown hardware budget (`0` = unknown).
///
/// This is the portable fallback used when no native backend (`mf`,
/// `mediacodec`) is compiled in or enumeration fails — it never claims hardware
/// it hasn't verified.
pub struct SoftwareCapsDiscovery;

impl CapsDiscovery for SoftwareCapsDiscovery {
    fn discover(&self) -> CapsMatrix {
        let software = |codec: Codec| CodecCaps {
            codec,
            profile: String::new(),
            level: String::new(),
            max_width: 4096,
            max_height: 2160,
            max_fps: 60,
            pixel_formats: vec![PixelFormat::Yuv420p],
            hw_accelerated: false,
        };

        CapsMatrix {
            codecs: vec![
                software(Codec::H264),
                software(Codec::Hevc),
                software(Codec::Vp9),
                software(Codec::Av1),
            ],
            // Unknown budget: `0` means the slot manager treats it as unbounded.
            max_hw_decoder_instances: 0,
            max_hw_encoder_instances: 0,
            verified_on_hardware: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_discovery_never_claims_hardware() {
        let m = SoftwareCapsDiscovery.discover();
        assert!(!m.verified_on_hardware);
        assert_eq!(m.max_hw_decoder_instances, 0);
        assert!(!m.hw_decodes(Codec::H264)); // software-only, not hw_accelerated
        assert!(m.codec(Codec::H264).is_some());
        assert!(m.codec(Codec::Av1).is_some());
    }
}
