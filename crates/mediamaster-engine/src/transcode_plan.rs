//! `TranscodePlan` — capability-decision: produce a target transcoding profile
//! from a source description and the device's `CapsMatrix` (from
//! `mediamaster_core`).
//!
//! The idea: a transcode pipeline needs to know *what* to produce and *how* to
//! produce it.  The source side only needs to say "what exists" (codec, container,
//! resolution, frame rate); the device side says "what is actually usable on the
//! destination" (`CapsMatrix` encodes decoder/encoder capability, per-resolution
//! limits, hardware-acceleration flags).  `TranscodePlan` marries the two.
//!
//! Note: `Codec` is the *source* codec vocabulary (H.264 / AVC, H.265 / HEVC,
//! AV1).  The device side uses `mediamaster_core::Codec`; they share the same
//! variant names and `for_device` decides the source codec from the device's
//! capability matrix.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

pub enum Container {
    /// MP4 (ISO-BMFF).
    Mp4,
    /// MPEG-TS.
    Ts,
    /// Matroska / WebM.
    Mkv,
}

/// A concrete transcoding target produced by `TranscodePlan::for_device`.
pub struct Profile {
    pub codec: Codec,
    pub container: Container,
    pub target_bitrate_bps: u64,
    pub max_frame_rate: u32,
    pub hardware_accelerated: bool,
}

/// A concrete transcode plan (what to encode/convert + how + the evidence that
/// says why).
pub struct TranscodePlan {
    pub profile: Profile,
    /// Output resolution.
    pub resolution: (u32, u32),
    /// Link throughput in bps if known, used for pacing and tier adaptation.
    pub link_throughput: Option<u64>,
    /// Human-readable reason (e.g. "HW-accelerated NVENC is available").
    pub why: String,
}

/// A typed HLS encoding job produced by `TranscodePlan::to_hls_job`.
#[cfg(feature = "hls")]
pub struct HlsJob {
    pub engine: crate::engine_selector::Engine,
    pub tier: mediamaster_core::DegradationTier,
    pub rendition: crate::hls::RenditionSpec,
    pub segmenter_config: crate::hls::HlsSegmenterConfig,
}

impl TranscodePlan {
    /// Decide the best transcode plan for a source of `width x height`, given
    /// the device's `CapsMatrix` (decoder + encoder capability, per-resolution
    /// limits, hardware flags).
    ///
    /// # Arguments
    /// * `width` — source width.
    /// * `height` — source height.
    /// * `caps` — the device's `mediamaster_core::CapsMatrix`.
    pub fn for_device(width: u32, height: u32, caps: &mediamaster_core::CapsMatrix) -> Self {
        let (width, height) = TranscodePlan::clamp_resolution(width, height);

        // Pick a hardware-capable codec: prefer the fastest that the device
        // actually advertises.  If hardware is not available, fall back to software.
        let (codec, hardware): (Codec, bool) = if caps.codecs.iter().any(|c| {
            matches!(
                c.codec,
                mediamaster_core::Codec::H264
                    | mediamaster_core::Codec::Hevc
                    | mediamaster_core::Codec::Av1
            )
        }) {
            // Prefer the most capable hardware codec that exists on both sides.
            if caps
                .codecs
                .iter()
                .any(|c| matches!(c.codec, mediamaster_core::Codec::Hevc))
            {
                (Codec::Hevc, true)
            } else {
                (Codec::H264, true)
            }
        } else {
            (Codec::H264, false)
        };

        let profile = Profile {
            codec,
            container: Container::Mp4,
            target_bitrate_bps: TranscodePlan::bitrate_for(width, height, codec),
            max_frame_rate: TranscodePlan::frame_rate(width, height),
            hardware_accelerated: hardware,
        };

        Self {
            profile,
            resolution: (width, height),
            link_throughput: None,
            why: TranscodePlan::why(hardware, codec),
        }
    }

    /// Builder method to attach link throughput pacing to the transcode plan.
    pub fn with_link_throughput(mut self, link_throughput: Option<u64>) -> Self {
        self.link_throughput = link_throughput;
        self
    }

    /// Decide the best transcode plan for a probed source (live stream or file asset)
    /// described by a `mediamaster_probe::MediaDescriptor`.
    pub fn for_descriptor(
        descriptor: &mediamaster_probe::MediaDescriptor,
        caps: &mediamaster_core::CapsMatrix,
    ) -> Option<Self> {
        let video = descriptor.video.as_ref()?;
        let width = video.width?;
        let height = video.height?;
        Some(Self::for_device(width, height, caps))
    }

    /// Produce a typed [`HlsJob`] routed through an [`EngineSelector`], acquiring
    /// hardware permits from a [`mediamaster_codec::CodecSlotManager`] with graceful fallback.
    #[cfg(feature = "hls")]
    pub fn to_hls_job(
        &self,
        selector: &dyn crate::engine_selector::EngineSelector,
        slots: &mut mediamaster_codec::CodecSlotManager,
        caps: &mediamaster_core::CapsMatrix,
        container: crate::hls::HlsContainer,
    ) -> HlsJob {
        let (engine, tier) = selector.select_encoder_with_tier(
            caps,
            self.profile.codec,
            self.link_throughput,
            slots,
        );

        let core_codec = match self.profile.codec {
            Codec::H264 => mediamaster_core::Codec::H264,
            Codec::Hevc => mediamaster_core::Codec::Hevc,
            Codec::Av1 => mediamaster_core::Codec::Av1,
        };

        let rendition = crate::hls::RenditionSpec::new(
            format!("{}p", self.resolution.1),
            self.resolution.0,
            self.resolution.1,
            self.profile.target_bitrate_bps,
            core_codec,
            container,
        );

        let segmenter_config = crate::hls::HlsSegmenterConfig {
            target_duration_ms: 2000,
            container,
            low_latency: false,
            part_target_duration_ms: 500,
            segment_prefix: format!("stream_{}p", self.resolution.1),
            width: self.resolution.0,
            height: self.resolution.1,
            bitrate_bps: self.profile.target_bitrate_bps,
        };

        HlsJob {
            engine,
            tier,
            rendition,
            segmenter_config,
        }
    }

    fn clamp_resolution(width: u32, height: u32) -> (u32, u32) {
        // Bound to the largest resolution the device reports as portable-safe
        // (≤ 4096).  The caller should produce a smaller source; this clamp is a
        // sanity floor against malformed sources.
        (width.min(4096), height.min(4096))
    }

    fn bitrate_for(width: u32, height: u32, codec: Codec) -> u64 {
        // Simple heuristic: 200–600 kbps per MP, scaled by resolution.
        let (w, h) = (
            width.clamp(16, 4096) as f64 / 16.0,
            height.clamp(16, 4096) as f64 / 16.0,
        );
        let mp = ((w * h) / 256.0).max(1.0);
        // kbps per MP (H.264 ≈ 300, HEVC ≈ 500), clamped 100–4000.
        let kbps =
            ((if codec == Codec::H264 { 300.0 } else { 500.0 } * mp).clamp(100.0, 4000.0)) as u64;
        kbps * 1000
    }

    fn frame_rate(width: u32, height: u32) -> u32 {
        if width <= 480 && height <= 480 {
            30
        } else if width <= 1920 && height <= 1080 {
            60
        } else {
            30
        }
    }

    fn why(hardware: bool, codec: Codec) -> String {
        if hardware {
            format!(
                "HW-accelerated {} is available",
                match codec {
                    Codec::H264 => "H.264",
                    Codec::Hevc => "H.265 (HEVC)",
                    Codec::Av1 => "AV1",
                }
            )
        } else {
            "Software fallback (no hardware acceleration)".to_string()
        }
    }
}
