//! `TranscodePlan` — capability-decision: produce a target transcoding profile
//! from a source description and the device's `CapsMatrix` (from
//! `mediamaster_codec`).
//!
//! The idea: a transcode pipeline needs to know *what* to produce and *how* to
//! produce it.  The source side only needs to say "what exists" (codec, container,
//! resolution, frame rate); the device side says "what is actually usable on the
//! destination" (`CapsMatrix` encodes decoder/encoder capability, per-resolution
//! limits, hardware-acceleration flags).  `TranscodePlan` marries the two.

pub enum Codec {
    /// H.264 / AVC.
    H264,
    /// H.265 / HEVC.
    Hevc,
    /// AV1.
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
    /// Human-readable reason (e.g. "HW-accelerated NVENC is available").
    pub why: String,
}

impl TranscodePlan {
    /// Decide the best transcode plan for a source of `width x height`, given
    /// the device's `CapsMatrix` (decoder + encoder capability, per-resolution
    /// limits, hardware flags).
    ///
    /// # Arguments
    /// * `width` — source width.
    /// * `height` — source height.
    /// * `caps` — the device's `mediamaster_codec::CapsMatrix`.
    pub fn for_device(width: u32, height: u32, caps: &mediamaster_codec::CapsMatrix) -> Self {
        let (width, height) = TranscodePlan::clamp_resolution(width, height);

        // Pick a hardware-capable codec: prefer the fastest that the device
        // actually advertises (decoder + encoder support).  If hardware is not
        // available, fall back to software.
        let (codec, hardware): (Codec, bool) = if caps.decoder.iter().any(|c| {
            matches!(
                c,
                mediamaster_codec::Codec::H264 | mediamaster_codec::Codec::Hevc
            )
        }) || caps.encoder.iter().any(|c| {
            matches!(
                c,
                mediamaster_codec::Codec::H264 | mediamaster_codec::Codec::Hevc
            )
        }) {
            // Prefer the most capable hardware codec that exists on both sides.
            if caps
                .encoder
                .iter()
                .any(|c| matches!(c, mediamaster_codec::Codec::Hevc))
                && caps
                    .decoder
                    .iter()
                    .any(|c| matches!(c, mediamaster_codec::Codec::Hevc))
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
            why: TranscodePlan::why(hardware, codec),
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
        let (w, h) = width.max(16).min(4096) as f64 / 16.0;
        let (h, _) = (height.max(16).min(4096) as f64 / 16.0, 0.0);
        let mp = (w * h / 256.0).max(1.0);
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
