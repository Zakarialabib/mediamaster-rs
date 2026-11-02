//! Unified RAG Facade & Context Envelope Integration.
//!
//! Provides `MediamasterRagFacade` unifying `CodecKnowledgeStore`,
//! `SceneMemoryStore`, and `PlaybookStore`. Outputs `RagContextEnvelope` for
//! host LLM/orchestration ingestion.

use serde::{Deserialize, Serialize};

use crate::codec_corpus::{CodecKnowledgeStore, CodecQuery, CodecRecommendation};
use crate::error::RagError;
use crate::playbooks::{AcousticProfile, PagingDuckingPlaybook, PlaybookStore, TriggerPattern};
use crate::scene_memory::{AtmospherePlaybook, SceneMemoryStore, SceneQuery};

/// Cross-pillar unified context envelope for consumption by desktop hosts
/// (e.g. `SignageMaster`) merging CCTV, TV-RS, and Mediamaster context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RagContextEnvelope {
    pub source: String,
    pub timestamp_ms: u64,
    pub codec_context: Option<CodecRecommendation>,
    pub atmosphere_context: Option<AtmospherePlaybook>,
    pub paging_context: Option<PagingDuckingPlaybook>,
    pub prompt_summary: String,
}

/// Unified facade exposing all knowledge store domains.
#[derive(Debug, Clone)]
pub struct MediamasterRagFacade {
    pub codecs: CodecKnowledgeStore,
    pub scenes: SceneMemoryStore,
    pub playbooks: PlaybookStore,
}

impl MediamasterRagFacade {
    /// Create a new facade with specified capacities for codec telemetry and scene memory.
    pub fn new(codec_capacity: usize, scene_capacity: usize) -> Self {
        Self {
            codecs: CodecKnowledgeStore::new(codec_capacity),
            scenes: SceneMemoryStore::new(scene_capacity),
            playbooks: PlaybookStore::new(),
        }
    }

    /// Query hardware codec degradation playbook.
    pub fn query_codec(&self, query: &CodecQuery) -> Result<CodecRecommendation, RagError> {
        self.codecs.query_codec_playbook(query)
    }

    /// Query environmental scene atmosphere playbook.
    pub fn query_scene(&self, query: &SceneQuery) -> Result<AtmospherePlaybook, RagError> {
        self.scenes.query_scene_memory(query)
    }

    /// Query audio paging/ducking playbook.
    pub fn query_playbook(
        &self,
        trigger: &TriggerPattern,
        acoustic: Option<&AcousticProfile>,
    ) -> Result<PagingDuckingPlaybook, RagError> {
        self.playbooks.query_playbook(trigger, acoustic)
    }

    /// Synthesize a unified `RagContextEnvelope` across all domains.
    pub fn build_context_envelope(
        &self,
        timestamp_ms: u64,
        codec_query: Option<&CodecQuery>,
        scene_query: Option<&SceneQuery>,
        trigger: Option<(&TriggerPattern, Option<&AcousticProfile>)>,
    ) -> RagContextEnvelope {
        let codec_context = codec_query.and_then(|q| self.query_codec(q).ok());
        let atmosphere_context = scene_query.and_then(|q| self.query_scene(q).ok());
        let paging_context = trigger.and_then(|(tr, ac)| self.query_playbook(tr, ac).ok());

        let mut summary_parts = Vec::new();

        if let Some(c) = &codec_context {
            if c.requires_degradation {
                summary_parts.push(format!("CODEC DEGRADATION: {}", c.rationale));
            } else {
                summary_parts.push("CODEC: Within normal hardware capacity".into());
            }
        }

        if let Some(a) = &atmosphere_context {
            summary_parts.push(format!(
                "ATMOSPHERE [{}]: master {:.1}dB, preset '{}', crossfade {}ms",
                a.scene_label,
                a.audio_state.master_volume_db,
                a.audio_state.acoustic_preset,
                a.audio_state.crossfade_duration_ms
            ));
        }

        if let Some(p) = &paging_context {
            summary_parts.push(format!(
                "PAGING [{}]: target gain {:.3}, hold {}ms",
                p.name, p.default_ducking.target_gain, p.hold_time_ms
            ));
        }

        let prompt_summary = if summary_parts.is_empty() {
            "No active context retrieved".into()
        } else {
            summary_parts.join(" | ")
        };

        RagContextEnvelope {
            source: "mediamaster".into(),
            timestamp_ms,
            codec_context,
            atmosphere_context,
            paging_context,
            prompt_summary,
        }
    }
}
