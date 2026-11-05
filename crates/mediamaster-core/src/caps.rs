//! Capability vocabulary: what a host can *actually* decode/encode.
//!
//! `CapsMatrix` is the epistemic capability truth the spec describes — the OS
//! enumerates hardware codecs, quirks overlay known driver lies, and a
//! synthetic micro-benchmark verifies advertised throughput. Consumers read the
//! matrix before executing so nothing guesses at `-hwaccel` support.

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// A codec this ecosystem understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum Codec {
    H264,
    Hevc,
    Av1,
    Vp9,
    Mpeg2,
    Aac,
    Opus,
    Mp3,
    Flac,
}

impl Codec {
    /// Stable machine name (e.g. codec fourcc / c2 name fragment).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Av1 => "av1",
            Self::Vp9 => "vp9",
            Self::Mpeg2 => "mpeg2",
            Self::Aac => "aac",
            Self::Opus => "opus",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
        }
    }
}

/// A pixel / sample format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum PixelFormat {
    /// 8-bit 4:2:0 semi-planar (the common HW decode surface).
    Nv12,
    /// 10-bit 4:2:0 semi-planar (HDR10 / HEVC Main10).
    P010,
    /// Planar 8-bit 4:2:0 (software fallback).
    Yuv420p,
    /// RGB24 (poster / fallback blit).
    Rgb24,
}

/// One codec's advertised capability on a host.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct CodecCaps {
    pub codec: Codec,
    /// H.264/HEVC profile string (e.g. `"High"`, `"Main10"`); empty for others.
    pub profile: String,
    /// H.264/HEVC level (e.g. `"5.1"`); empty for others.
    pub level: String,
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u32,
    /// Pixel formats this codec can be fed / produced in.
    pub pixel_formats: Vec<PixelFormat>,
    /// `true` when backed by real hardware (not a software fallback).
    pub hw_accelerated: bool,
}

/// How a layout degrades when the hardware cannot meet its demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum DegradationTier {
    /// Full-rate hardware surface compositing.
    Tier1HwSurface,
    /// Hardware decode into byte buffers (slower blit).
    Tier2HwByteBuffer,
    /// Shared-texture mux (cross-process, higher latency).
    Tier3SharedTextureMux,
    /// Software / poster fallback (last resort, may drop to stills).
    Tier4PosterFallback,
}

/// The full capability matrix for a host: every codec it advertises, plus the
/// hardware instance budget.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct CapsMatrix {
    pub codecs: Vec<CodecCaps>,
    /// Concurrent hardware decoder instances the SoC supports (Android MediaCodec
    /// slot exhaustion guard). `0` = unknown.
    pub max_hw_decoder_instances: u8,
    /// Concurrent hardware encoder instances. `0` = unknown.
    pub max_hw_encoder_instances: u8,
    /// Confidence that the rows were verified on hardware (vs. only enumerated).
    pub verified_on_hardware: bool,
}

impl CapsMatrix {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a codec's capability, if present.
    pub fn codec(&self, codec: Codec) -> Option<&CodecCaps> {
        self.codecs.iter().find(|c| c.codec == codec)
    }

    /// `true` when this host can hardware-decode `codec`.
    pub fn hw_decodes(&self, codec: Codec) -> bool {
        self.codec(codec).is_some_and(|c| c.hw_accelerated)
    }
}

/// Verdict on an observed capability based on runtime performance/telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum CapabilityVerdict {
    /// Confirmed capable under observed conditions.
    Confirmed,
    /// Borderline stability (intermittent frame drops or near threshold).
    Borderline,
    /// Refuted: hardware failed or repeatedly starved under load.
    Refuted,
}

/// Runtime observed capability of a codec on a hardware platform.
///
/// Records empirical decode/encode metrics across streams to detect and
/// compensate for hardware driver lies.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct ObservedCodecCaps {
    /// SoC/GPU family hash or class (NOT serial numbers — privacy).
    pub device_class: String,
    pub codec: Codec,
    pub profile: String,
    pub level: String,
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u32,
    pub streams_concurrently_observed: u32,
    pub starvation_events: u32,
    pub verdict: CapabilityVerdict,
    /// Empirical confidence score in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Observation window start in epoch milliseconds.
    pub observed_at_start_ms: u64,
    /// Observation window end in epoch milliseconds.
    pub observed_at_end_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_matrix() -> CapsMatrix {
        CapsMatrix {
            codecs: vec![CodecCaps {
                codec: Codec::Hevc,
                profile: "Main10".into(),
                level: "5.1".into(),
                max_width: 3840,
                max_height: 2160,
                max_fps: 60,
                pixel_formats: vec![PixelFormat::P010, PixelFormat::Nv12],
                hw_accelerated: true,
            }],
            max_hw_decoder_instances: 2,
            max_hw_encoder_instances: 1,
            verified_on_hardware: true,
        }
    }

    #[test]
    fn codec_lookup_and_hw_decode() {
        let m = sample_matrix();
        assert!(m.hw_decodes(Codec::Hevc));
        assert!(!m.hw_decodes(Codec::H264));
        assert_eq!(m.codec(Codec::Hevc).unwrap().profile, "Main10");
        assert_eq!(m.max_hw_decoder_instances, 2);
    }
}
