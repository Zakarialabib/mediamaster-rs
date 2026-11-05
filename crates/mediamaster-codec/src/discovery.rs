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

/// Known driver quirk rule that overrides advertised capabilities with ground truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuirkRule {
    /// Affected codec.
    pub codec: Codec,
    /// Whether hardware acceleration should be disallowed (e.g. broken driver).
    pub disable_hw: bool,
    /// Max decoder instances override if capped by driver bugs.
    pub max_hw_decoder_override: Option<u8>,
}

/// Capability discovery that layers known driver quirks and telemetry overrides
/// over a raw [`CapsDiscovery`] source.
pub struct QuirksAwareDiscovery<D: CapsDiscovery> {
    inner: D,
    quirks: Vec<QuirkRule>,
}

impl<D: CapsDiscovery> QuirksAwareDiscovery<D> {
    pub fn new(inner: D) -> Self {
        Self {
            inner,
            quirks: Vec::new(),
        }
    }

    pub fn with_quirks(inner: D, quirks: Vec<QuirkRule>) -> Self {
        Self { inner, quirks }
    }

    pub fn add_quirk(&mut self, quirk: QuirkRule) {
        self.quirks.push(quirk);
    }
}

impl<D: CapsDiscovery> CapsDiscovery for QuirksAwareDiscovery<D> {
    fn discover(&self) -> CapsMatrix {
        let mut matrix = self.inner.discover();

        for quirk in &self.quirks {
            if quirk.disable_hw {
                for c in &mut matrix.codecs {
                    if c.codec == quirk.codec {
                        c.hw_accelerated = false;
                    }
                }
            }
            if let Some(max_dec) = quirk.max_hw_decoder_override {
                matrix.max_hw_decoder_instances = max_dec;
            }
        }

        matrix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quirks_aware_discovery_overrides_matrix() {
        struct MockDiscovery;
        impl CapsDiscovery for MockDiscovery {
            fn discover(&self) -> CapsMatrix {
                CapsMatrix {
                    codecs: vec![CodecCaps {
                        codec: Codec::Hevc,
                        profile: "Main10".into(),
                        level: "5.1".into(),
                        max_width: 3840,
                        max_height: 2160,
                        max_fps: 60,
                        pixel_formats: vec![PixelFormat::P010],
                        hw_accelerated: true,
                    }],
                    max_hw_decoder_instances: 4,
                    max_hw_encoder_instances: 1,
                    verified_on_hardware: true,
                }
            }
        }

        let quirks = vec![QuirkRule {
            codec: Codec::Hevc,
            disable_hw: true,
            max_hw_decoder_override: Some(2),
        }];
        let disco = QuirksAwareDiscovery::with_quirks(MockDiscovery, quirks);
        let m = disco.discover();
        assert!(!m.codec(Codec::Hevc).unwrap().hw_accelerated);
        assert_eq!(m.max_hw_decoder_instances, 2);
    }

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
