//! Hardware Codec & Starvation Corpus.
//!
//! Indexes hardware profiles (e.g., "Windows Media Foundation on 8GB VRAM supports
//! max 3 concurrent 4K60 HEVC 10-bit hardware decode slots before stepping down
//! to DegradationTier::SubStream").
//!
//! Evaluates intended stream layouts against historical crash/starvation
//! telemetry for the host's hardware fingerprint, returning proactive
//! degradation recommendations before the GPU/decoder stalls.

use mediamaster_core::caps::{CapsMatrix, Codec, DegradationTier};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::error::RagError;

/// Host hardware fingerprint describing OS, graphics/SoC architecture, and memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HardwareFingerprint {
    pub os: String,
    pub gpu_soc: String,
    pub vram_mb: u32,
    pub driver_version: String,
}

impl HardwareFingerprint {
    pub fn new(
        os: impl Into<String>,
        gpu_soc: impl Into<String>,
        vram_mb: u32,
        driver_version: impl Into<String>,
    ) -> Self {
        Self {
            os: os.into(),
            gpu_soc: gpu_soc.into(),
            vram_mb,
            driver_version: driver_version.into(),
        }
    }

    /// Similarity match between host fingerprints.
    pub fn matches(&self, other: &HardwareFingerprint) -> bool {
        self.os.eq_ignore_ascii_case(&other.os)
            && self.gpu_soc.eq_ignore_ascii_case(&other.gpu_soc)
            && self.vram_mb == other.vram_mb
    }
}

/// Description of an individual media stream in a proposed layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDescriptor {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bit_depth: u8,
}

impl StreamDescriptor {
    pub fn new(codec: Codec, width: u32, height: u32, fps: u32, bit_depth: u8) -> Self {
        Self {
            codec,
            width,
            height,
            fps,
            bit_depth,
        }
    }

    /// Whether this stream is high-demand (e.g. 4K60 or above).
    pub fn is_4k(&self) -> bool {
        self.width >= 3840 && self.height >= 2160
    }
}

/// A target layout of multiple concurrent video streams (e.g., video wall panels).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StreamLayout {
    pub streams: Vec<StreamDescriptor>,
}

impl StreamLayout {
    pub fn new(streams: Vec<StreamDescriptor>) -> Self {
        Self { streams }
    }

    pub fn count_4k_hevc(&self) -> usize {
        self.streams
            .iter()
            .filter(|s| s.codec == Codec::Hevc && s.is_4k())
            .count()
    }
}

/// Historical starvation and bottleneck telemetry for a layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StarvationTelemetry {
    pub dropped_frames_per_sec: f32,
    pub jitter_ms: f32,
    pub decoder_starvation_events: u32,
    pub surface_allocation_failures: u32,
}

/// Actionable degradation recommendation for a stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DegradationAction {
    pub stream_index: usize,
    pub target_tier: DegradationTier,
    /// When stepping down to SubStream, recommended resolution/fps reduction.
    pub substream_target: Option<StreamDescriptor>,
    pub reason: String,
}

/// A recorded entry in the knowledge store corpus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodecCorpusEntry {
    pub fingerprint: HardwareFingerprint,
    pub capability_bundle: CapsMatrix,
    pub observed_layout: StreamLayout,
    pub telemetry: StarvationTelemetry,
    pub recommended_degradations: Vec<DegradationAction>,
}

/// Query to determine proactive degradation for a proposed stream layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodecQuery {
    pub fingerprint: HardwareFingerprint,
    pub capability_bundle: Option<CapsMatrix>,
    pub proposed_layout: StreamLayout,
}

/// Recommendation response returned to `mediamaster-agent` or layout orchestrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecRecommendation {
    pub requires_degradation: bool,
    pub recommended_actions: Vec<DegradationAction>,
    pub rationale: String,
}

/// Bounded knowledge store for hardware codec capabilities and starvation logs.
#[derive(Debug, Clone)]
pub struct CodecKnowledgeStore {
    capacity: usize,
    entries: VecDeque<CodecCorpusEntry>,
}

impl CodecKnowledgeStore {
    /// Create a new store with a bounded maximum capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    /// Current count of entries in the store.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record a telemetry and degradation profile entry, evicting oldest if capacity is reached.
    pub fn record_telemetry(&mut self, entry: CodecCorpusEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Query the knowledge store for degradation recommendations.
    pub fn query_codec_playbook(
        &self,
        query: &CodecQuery,
    ) -> Result<CodecRecommendation, RagError> {
        if self.entries.is_empty() {
            return Err(RagError::EmptyStore);
        }

        // 1. Find matching entries for this hardware fingerprint
        let matching_entries: Vec<&CodecCorpusEntry> = self
            .entries
            .iter()
            .filter(|e| e.fingerprint.matches(&query.fingerprint))
            .collect();

        if matching_entries.is_empty() {
            return Err(RagError::NoMatchFound);
        }

        // 2. Evaluate proposed layout against historical starvation
        let proposed_4k_hevc = query.proposed_layout.count_4k_hevc();
        let total_streams = query.proposed_layout.streams.len();

        let mut starvation_observed = false;
        let mut actions = Vec::new();
        let mut rationale = String::new();

        for entry in matching_entries {
            let entry_4k_hevc = entry.observed_layout.count_4k_hevc();
            let entry_total = entry.observed_layout.streams.len();

            let is_starving = entry.telemetry.decoder_starvation_events > 0
                || entry.telemetry.surface_allocation_failures > 0
                || entry.telemetry.dropped_frames_per_sec > 5.0;

            if is_starving && (proposed_4k_hevc >= entry_4k_hevc || total_streams >= entry_total) {
                starvation_observed = true;
                rationale = format!(
                    "Hardware '{}' experienced starvation/allocation failure ({} dropped fps, {} starvation events) on {}x 4K HEVC streams. Recommending degradation to SubStream.",
                    query.fingerprint.gpu_soc,
                    entry.telemetry.dropped_frames_per_sec,
                    entry.telemetry.decoder_starvation_events,
                    entry_4k_hevc
                );

                for rec in &entry.recommended_degradations {
                    if rec.stream_index < total_streams
                        && !actions
                            .iter()
                            .any(|a: &DegradationAction| a.stream_index == rec.stream_index)
                    {
                        actions.push(rec.clone());
                    }
                }
            }
        }

        if !starvation_observed {
            // Check CapsMatrix max_hw_decoder_instances if provided
            if let Some(caps) = &query.capability_bundle {
                let max_hw = caps.max_hw_decoder_instances as usize;
                if max_hw > 0 && total_streams > max_hw {
                    let mut fallback_actions = Vec::new();
                    for idx in max_hw..total_streams {
                        fallback_actions.push(DegradationAction {
                            stream_index: idx,
                            target_tier: DegradationTier::Tier4PosterFallback,
                            substream_target: None,
                            reason: format!(
                                "Exceeds hardware decoder instance limit of {max_hw} concurrent streams"
                            ),
                        });
                    }
                    return Ok(CodecRecommendation {
                        requires_degradation: true,
                        recommended_actions: fallback_actions,
                        rationale: format!(
                            "Layout requests {total_streams} streams but host CapsMatrix limits to {max_hw} HW decoders"
                        ),
                    });
                }
            }

            return Ok(CodecRecommendation {
                requires_degradation: false,
                recommended_actions: Vec::new(),
                rationale:
                    "Proposed layout is within verified hardware envelope without starvation".into(),
            });
        }

        // Ensure actions are sorted by stream index
        actions.sort_by_key(|a| a.stream_index);

        Ok(CodecRecommendation {
            requires_degradation: !actions.is_empty(),
            recommended_actions: actions,
            rationale,
        })
    }
}
