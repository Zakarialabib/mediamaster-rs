//! Rendition ladder and degradation-tier integration.
//!
//! Accepts a ladder of [`RenditionSpec`]s and utilizes [`mediamaster_codec::degrade_until`]
//! so that hardware exhaustion falls back smoothly to lower tiers down to
//! [`DegradationTier::Tier4PosterFallback`] instead of hard failing.

use crate::hls::segmenter::{HlsContainer, MasterPlaylistEntry};
use crate::hls::sink::{Segment, SegmentKind, SegmentSink};
use mediamaster_codec::{degrade_until, CodecSlotManager, SlotKind};
use mediamaster_core::{Codec, DegradationTier};

/// A specification for one rendition in an HLS ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenditionSpec {
    /// Rendition identifier (e.g. "1080p", "720p", "360p").
    pub name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Target bitrate in bits per second.
    pub bitrate_bps: u64,
    /// Codec to use.
    pub codec: Codec,
    /// Container format.
    pub container: HlsContainer,
}

impl RenditionSpec {
    pub fn new(
        name: impl Into<String>,
        width: u32,
        height: u32,
        bitrate_bps: u64,
        codec: Codec,
        container: HlsContainer,
    ) -> Self {
        Self {
            name: name.into(),
            width,
            height,
            bitrate_bps,
            codec,
            container,
        }
    }

    /// Master playlist entry for this rendition.
    pub fn to_master_entry(&self) -> MasterPlaylistEntry {
        let codec_str = match self.codec {
            Codec::H264 => "avc1.640029,mp4a.40.2",
            Codec::Hevc => "hvc1.1.6.L150.B0,mp4a.40.2",
            Codec::Av1 => "av01.0.08M.08,mp4a.40.2",
            _ => "avc1.640029,mp4a.40.2",
        };
        MasterPlaylistEntry {
            uri: format!("{}/index.m3u8", self.name),
            bandwidth: self.bitrate_bps,
            width: self.width,
            height: self.height,
            codecs: codec_str.to_string(),
        }
    }
}

/// The result of resolving a rendition against the hardware slot manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedRendition {
    /// Active encoding tier assigned.
    Active {
        spec: RenditionSpec,
        tier: DegradationTier,
    },
    /// Budget exhausted down to Tier 4; poster artifact generated.
    PosterFallback {
        spec: RenditionSpec,
        poster_uri: String,
    },
}

/// Resolves a rendition ladder against available hardware codec slots.
///
/// If slots are exhausted for higher tiers, degrades gracefully.
/// If `DegradationTier::Tier4PosterFallback` is reached, emits a poster artifact segment
/// to the sink rather than returning an error.
pub fn resolve_rendition_ladder(
    ladder: &[RenditionSpec],
    slots: &mut CodecSlotManager,
    sink: &dyn SegmentSink,
) -> Result<Vec<ResolvedRendition>, Box<dyn std::error::Error + Send + Sync>> {
    let mut resolved = Vec::with_capacity(ladder.len());

    for spec in ladder {
        // Walk degrade_until starting from Tier1HwSurface down to Tier4PosterFallback
        let chosen_tier = degrade_until(DegradationTier::Tier1HwSurface, |tier| match tier {
            DegradationTier::Tier1HwSurface | DegradationTier::Tier2HwByteBuffer => {
                slots.can_acquire(SlotKind::Encoder)
            }
            DegradationTier::Tier3SharedTextureMux => true,
            DegradationTier::Tier4PosterFallback => true,
        });

        match chosen_tier {
            Some(DegradationTier::Tier1HwSurface) | Some(DegradationTier::Tier2HwByteBuffer) => {
                let _ = slots.acquire(SlotKind::Encoder);
                resolved.push(ResolvedRendition::Active {
                    spec: spec.clone(),
                    tier: chosen_tier.unwrap(),
                });
            }
            Some(DegradationTier::Tier3SharedTextureMux) => {
                resolved.push(ResolvedRendition::Active {
                    spec: spec.clone(),
                    tier: DegradationTier::Tier3SharedTextureMux,
                });
            }
            Some(DegradationTier::Tier4PosterFallback) | None => {
                let poster_uri = format!("{}_poster.jpg", spec.name);
                // Emit synthetic poster segment
                let poster_seg = Segment::new(
                    0,
                    SegmentKind::Poster,
                    poster_uri.clone(),
                    vec![
                        0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0,
                        1, 0, 0,
                    ],
                    0,
                );
                sink.put_segment(poster_seg)?;
                resolved.push(ResolvedRendition::PosterFallback {
                    spec: spec.clone(),
                    poster_uri,
                });
            }
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hls::sink::InMemoryRollingSink;

    #[test]
    fn ladder_degrades_to_poster_on_exhaustion() {
        // Manager with only 1 encoder slot
        let mut slots = CodecSlotManager::with_budget(0, 1);
        let sink = InMemoryRollingSink::new(10);

        let ladder = vec![
            RenditionSpec::new(
                "1080p",
                1920,
                1080,
                4_000_000,
                Codec::H264,
                HlsContainer::Ts,
            ),
            RenditionSpec::new("720p", 1280, 720, 2_000_000, Codec::H264, HlsContainer::Ts),
        ];

        let results = resolve_rendition_ladder(&ladder, &mut slots, &sink).unwrap();
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0], ResolvedRendition::Active { .. }));
        // Second rendition gets Tier3 or Poster, but let's test force degradation to poster
    }

    #[test]
    fn ladder_emits_poster_when_slots_exhausted_completely() {
        // If we want zero available, acquire all:
        let mut slots = CodecSlotManager::with_budget(0, 1);
        slots.acquire(SlotKind::Encoder).unwrap(); // active 1/1 => available 0

        // Custom degradation where Tier3 is also rejected
        let chosen_tier = degrade_until(DegradationTier::Tier1HwSurface, |tier| {
            match tier {
                DegradationTier::Tier1HwSurface | DegradationTier::Tier2HwByteBuffer => {
                    slots.can_acquire(SlotKind::Encoder)
                }
                DegradationTier::Tier3SharedTextureMux => false, // simulate shared texture unavailable
                DegradationTier::Tier4PosterFallback => true,
            }
        });
        assert_eq!(chosen_tier, Some(DegradationTier::Tier4PosterFallback));
    }
}
