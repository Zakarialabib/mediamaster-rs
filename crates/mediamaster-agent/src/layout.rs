//! Layout orchestration with proactive RAG codec capability verification.
//!
//! Evaluates multi-stream layout requests against host [`CapsMatrix`] and
//! historical starvation knowledge in [`MediamasterRagFacade`]. Proactively
//! steps down stream tiers ([`DegradationTier`]) before streams are activated
//! when starvation is predicted or instance budgets are exceeded.

use mediamaster_codec::slot_budget::{degrade_until, CodecSlotManager, SlotKind};
use mediamaster_core::caps::{CapsMatrix, DegradationTier};
use mediamaster_rag::{
    CodecQuery, DegradationAction, HardwareFingerprint, MediamasterRagFacade, StreamDescriptor,
    StreamLayout,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Execution plan for an activated multi-stream layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveStreamAssignment {
    pub stream_index: usize,
    pub descriptor: StreamDescriptor,
    pub tier: DegradationTier,
    pub action: Option<DegradationAction>,
}

/// A complete layout plan after evaluation against hardware capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLayoutPlan {
    pub assignments: Vec<ActiveStreamAssignment>,
    pub requires_degradation: bool,
    pub rationale: String,
}

/// Orchestrates multi-stream layout activation, guarding against hardware slot exhaustion
/// and GPU/VRAM starvation using `mediamaster-rag`.
#[derive(Clone)]
pub struct LayoutOrchestrator {
    caps: CapsMatrix,
    fingerprint: HardwareFingerprint,
    rag: Arc<RwLock<MediamasterRagFacade>>,
}

impl LayoutOrchestrator {
    /// Create a new layout orchestrator.
    pub fn new(
        caps: CapsMatrix,
        fingerprint: HardwareFingerprint,
        rag: Arc<RwLock<MediamasterRagFacade>>,
    ) -> Self {
        Self {
            caps,
            fingerprint,
            rag,
        }
    }

    /// Access the host capability matrix.
    pub fn caps(&self) -> &CapsMatrix {
        &self.caps
    }

    /// Access the hardware fingerprint.
    pub fn fingerprint(&self) -> &HardwareFingerprint {
        &self.fingerprint
    }

    /// Evaluates a proposed layout against RAG knowledge and host capabilities,
    /// returning an [`ActiveLayoutPlan`] with proactive degradation applied.
    pub async fn evaluate_layout(&self, layout: &StreamLayout) -> ActiveLayoutPlan {
        let query = CodecQuery {
            fingerprint: self.fingerprint.clone(),
            capability_bundle: Some(self.caps.clone()),
            proposed_layout: layout.clone(),
        };

        // Query RAG facade asynchronously
        let rec_result = {
            let facade = self.rag.read().await;
            facade.query_codec(&query)
        };

        let mut assignments = Vec::with_capacity(layout.streams.len());
        let mut slot_mgr = CodecSlotManager::new(&self.caps);
        let mut held_leases = Vec::new();

        match rec_result {
            Ok(recommendation) => {
                let mut requires_degradation = recommendation.requires_degradation;
                let rationale = recommendation.rationale;

                for (idx, stream) in layout.streams.iter().enumerate() {
                    // Check if RAG recommended a proactive degradation action
                    let action = recommendation
                        .recommended_actions
                        .iter()
                        .find(|a| a.stream_index == idx)
                        .cloned();

                    let (tier, descriptor) = if let Some(ref act) = action {
                        let desc = act
                            .substream_target
                            .clone()
                            .unwrap_or_else(|| stream.clone());
                        (act.target_tier, desc)
                    } else {
                        // Check slot manager budget
                        match slot_mgr.acquire(SlotKind::Decoder) {
                            Ok(lease) => {
                                held_leases.push(lease);
                                (DegradationTier::Tier1HwSurface, stream.clone())
                            }
                            Err(_) => {
                                // Slot exhausted: degrade tier
                                requires_degradation = true;
                                let chosen = degrade_until(DegradationTier::Tier1HwSurface, |t| {
                                    t > DegradationTier::Tier1HwSurface
                                })
                                .unwrap_or(DegradationTier::Tier4PosterFallback);
                                (chosen, stream.clone())
                            }
                        }
                    };

                    assignments.push(ActiveStreamAssignment {
                        stream_index: idx,
                        descriptor,
                        tier,
                        action,
                    });
                }

                ActiveLayoutPlan {
                    assignments,
                    requires_degradation,
                    rationale,
                }
            }
            Err(_) => {
                // No historical entry or empty store: enforce CapsMatrix slot budget directly
                let mut requires_degradation = false;
                for (idx, stream) in layout.streams.iter().enumerate() {
                    let tier = match slot_mgr.acquire(SlotKind::Decoder) {
                        Ok(lease) => {
                            held_leases.push(lease);
                            DegradationTier::Tier1HwSurface
                        }
                        Err(_) => {
                            requires_degradation = true;
                            DegradationTier::Tier4PosterFallback
                        }
                    };

                    assignments.push(ActiveStreamAssignment {
                        stream_index: idx,
                        descriptor: stream.clone(),
                        tier,
                        action: None,
                    });
                }

                ActiveLayoutPlan {
                    assignments,
                    requires_degradation,
                    rationale: "Default CapsMatrix hardware slot budgeting applied".into(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::caps::{Codec, CodecCaps, PixelFormat};
    use mediamaster_rag::{CodecCorpusEntry, StarvationTelemetry};

    fn make_test_caps(max_hw_decoders: u8) -> CapsMatrix {
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
            max_hw_decoder_instances: max_hw_decoders,
            max_hw_encoder_instances: 1,
            verified_on_hardware: true,
        }
    }

    #[tokio::test]
    async fn evaluate_layout_proactive_degradation_on_starvation() {
        let caps = make_test_caps(4);
        let fingerprint =
            HardwareFingerprint::new("Windows", "MediaFoundation_D3D11", 8192, "31.0.101");

        let mut rag_facade = MediamasterRagFacade::new(5, 5);

        // Telemetry indicating starvation on 4 concurrent 4K HEVC streams
        let layout_4k_4x = StreamLayout::new(vec![
            StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
            StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
            StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
            StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
        ]);

        let starvation_telemetry = StarvationTelemetry {
            dropped_frames_per_sec: 15.0,
            jitter_ms: 30.0,
            decoder_starvation_events: 5,
            surface_allocation_failures: 1,
        };

        let degradation_recs = vec![DegradationAction {
            stream_index: 3,
            target_tier: DegradationTier::Tier2HwByteBuffer,
            substream_target: Some(StreamDescriptor::new(Codec::H264, 1920, 1080, 60, 8)),
            reason: "Proactively degrade stream 3 to 1080p SubStream to prevent VRAM starvation"
                .into(),
        }];

        rag_facade.codecs.record_telemetry(CodecCorpusEntry {
            fingerprint: fingerprint.clone(),
            capability_bundle: caps.clone(),
            observed_layout: layout_4k_4x.clone(),
            telemetry: starvation_telemetry,
            recommended_degradations: degradation_recs,
        });

        let rag = Arc::new(RwLock::new(rag_facade));
        let orchestrator = LayoutOrchestrator::new(caps, fingerprint, rag);

        let plan = orchestrator.evaluate_layout(&layout_4k_4x).await;

        assert!(plan.requires_degradation);
        assert_eq!(plan.assignments.len(), 4);
        assert_eq!(plan.assignments[0].tier, DegradationTier::Tier1HwSurface);
        assert_eq!(plan.assignments[3].tier, DegradationTier::Tier2HwByteBuffer);
        assert_eq!(
            plan.assignments[3].descriptor,
            StreamDescriptor::new(Codec::H264, 1920, 1080, 60, 8)
        );
    }

    #[tokio::test]
    async fn evaluate_layout_caps_matrix_budget_exhaustion() {
        // Hardware allows at most 2 decoders
        let caps = make_test_caps(2);
        let fingerprint = HardwareFingerprint::new("Android", "Qualcomm_Adreno", 4096, "512.0");

        let rag_facade = MediamasterRagFacade::new(5, 5);
        let rag = Arc::new(RwLock::new(rag_facade));
        let orchestrator = LayoutOrchestrator::new(caps, fingerprint, rag);

        let layout_3x = StreamLayout::new(vec![
            StreamDescriptor::new(Codec::Hevc, 1920, 1080, 30, 8),
            StreamDescriptor::new(Codec::Hevc, 1920, 1080, 30, 8),
            StreamDescriptor::new(Codec::Hevc, 1920, 1080, 30, 8),
        ]);

        let plan = orchestrator.evaluate_layout(&layout_3x).await;

        assert!(plan.requires_degradation);
        assert_eq!(plan.assignments.len(), 3);
        assert_eq!(plan.assignments[0].tier, DegradationTier::Tier1HwSurface);
        assert_eq!(plan.assignments[1].tier, DegradationTier::Tier1HwSurface);
        // Third stream exceeds max_hw_decoder_instances: degraded
        assert!(plan.assignments[2].tier > DegradationTier::Tier1HwSurface);
    }
}
