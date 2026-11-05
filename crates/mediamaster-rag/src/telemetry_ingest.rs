//! Telemetry Ingest & Sliding-Window Codec Knowledge Synthesizer.
//!
//! Subscribes to bus events (`SlotRevoked`, `StarvationDetected`, `DecodeHeartbeat`)
//! and updates empirical confidence and verdict records in `CodecKnowledgeStore`
//! and `QuirksCorpus`.
//!
//! Applies sliding-window decay so older events lose weight over time.

use mediamaster_core::caps::{CapabilityVerdict, Codec, ObservedCodecCaps};
use mediamaster_core::events::HardwareEvent;
use std::collections::BTreeMap;

use crate::codec_corpus::{
    CodecCorpusEntry, CodecKnowledgeStore, HardwareFingerprint, StarvationTelemetry,
    StreamDescriptor, StreamLayout,
};
use crate::quirks::QuirksCorpus;

/// Telemetry observation accumulator for a specific `(device_class, codec)`.
#[derive(Debug, Clone)]
struct ObservationAccumulator {
    device_class: String,
    codec: Codec,
    max_width_observed: u32,
    max_height_observed: u32,
    max_fps_observed: u32,
    max_concurrent_streams: u32,
    /// Starvation events weighted by sliding-window decay.
    weighted_starvations: f32,
    /// Successful heartbeat duration ms weighted by sliding-window decay.
    weighted_heartbeat_ms: f64,
    /// Total raw starvation occurrences.
    raw_starvations: u32,
    first_seen_ms: u64,
    last_seen_ms: u64,
}

impl ObservationAccumulator {
    fn new(device_class: String, codec: Codec, timestamp_ms: u64) -> Self {
        Self {
            device_class,
            codec,
            max_width_observed: 0,
            max_height_observed: 0,
            max_fps_observed: 0,
            max_concurrent_streams: 0,
            weighted_starvations: 0.0,
            weighted_heartbeat_ms: 0.0,
            raw_starvations: 0,
            first_seen_ms: timestamp_ms,
            last_seen_ms: timestamp_ms,
        }
    }

    /// Apply exponential decay based on elapsed time since last event.
    fn decay(&mut self, current_ms: u64, half_life_ms: f64) {
        if current_ms > self.last_seen_ms && half_life_ms > 0.0 {
            let elapsed = (current_ms - self.last_seen_ms) as f64;
            let decay_factor = (-elapsed * std::f64::consts::LN_2 / half_life_ms).exp() as f32;
            self.weighted_starvations *= decay_factor;
            self.weighted_heartbeat_ms *= decay_factor as f64;
            self.last_seen_ms = current_ms;
        }
    }

    /// Compute current verdict and confidence.
    fn compute_verdict(&self, starvation_threshold: f32) -> (CapabilityVerdict, f32) {
        // Consider small float epsilon when evaluating threshold
        if self.weighted_starvations >= (starvation_threshold - 0.01) {
            let conf =
                (0.5 + (self.weighted_starvations / (starvation_threshold * 2.0)) * 0.5).min(1.0);
            (CapabilityVerdict::Refuted, conf)
        } else if self.weighted_starvations > 0.0 {
            let conf = 0.5;
            (CapabilityVerdict::Borderline, conf)
        } else if self.weighted_heartbeat_ms > 10_000.0 {
            let conf = (self.weighted_heartbeat_ms / 60_000.0).min(1.0) as f32;
            (CapabilityVerdict::Confirmed, conf.max(0.7))
        } else {
            (CapabilityVerdict::Confirmed, 0.5)
        }
    }

    fn to_observed_caps(&self, starvation_threshold: f32) -> ObservedCodecCaps {
        let (verdict, confidence) = self.compute_verdict(starvation_threshold);
        ObservedCodecCaps {
            device_class: self.device_class.clone(),
            codec: self.codec,
            profile: String::new(),
            level: String::new(),
            max_width: self.max_width_observed,
            max_height: self.max_height_observed,
            max_fps: self.max_fps_observed,
            streams_concurrently_observed: self.max_concurrent_streams,
            starvation_events: self.raw_starvations,
            verdict,
            confidence,
            observed_at_start_ms: self.first_seen_ms,
            observed_at_end_ms: self.last_seen_ms,
        }
    }
}

/// Ingests codec telemetry events, computes decaying observations, and mutates knowledge stores.
#[derive(Debug, Clone)]
pub struct TelemetryIngest {
    observations: BTreeMap<(String, Codec), ObservationAccumulator>,
    /// Starvation threshold (weighted count) required to transition to `Refuted`.
    starvation_threshold: f32,
    /// Half-life in milliseconds for observation decay (default: 1 hour = 3,600,000 ms).
    half_life_ms: f64,
}

impl TelemetryIngest {
    /// Create a new TelemetryIngest with default thresholds (3 starvations to refute, 1h half-life).
    pub fn new() -> Self {
        Self {
            observations: BTreeMap::new(),
            starvation_threshold: 3.0,
            half_life_ms: 3_600_000.0,
        }
    }

    /// Create with custom thresholds (useful for tests).
    pub fn with_thresholds(starvation_threshold: f32, half_life_ms: f64) -> Self {
        Self {
            observations: BTreeMap::new(),
            starvation_threshold,
            half_life_ms,
        }
    }

    /// Ingest a `HardwareEvent`, mutating both the `CodecKnowledgeStore` and `QuirksCorpus`.
    pub fn ingest_event(
        &mut self,
        event: &HardwareEvent,
        now_ms: u64,
        knowledge_store: &mut CodecKnowledgeStore,
        corpus: &mut QuirksCorpus,
    ) {
        match event {
            HardwareEvent::StarvationDetected {
                device_class,
                codec,
                width,
                height,
                fps,
                concurrent_streams,
            } => {
                let key = (device_class.to_lowercase(), *codec);
                let acc = match self.observations.get_mut(&key) {
                    Some(existing) => {
                        existing.decay(now_ms, self.half_life_ms);
                        existing
                    }
                    None => {
                        self.observations.insert(
                            key.clone(),
                            ObservationAccumulator::new(device_class.clone(), *codec, now_ms),
                        );
                        self.observations.get_mut(&key).unwrap()
                    }
                };

                acc.raw_starvations += 1;
                acc.weighted_starvations += 1.0;
                acc.max_width_observed = acc.max_width_observed.max(*width);
                acc.max_height_observed = acc.max_height_observed.max(*height);
                acc.max_fps_observed = acc.max_fps_observed.max(*fps);
                acc.max_concurrent_streams = acc.max_concurrent_streams.max(*concurrent_streams);

                let obs = acc.to_observed_caps(self.starvation_threshold);
                corpus.record_observation(&obs);

                // Record into CodecKnowledgeStore
                let fingerprint = HardwareFingerprint::new("generic", device_class, 0, "unknown");
                let layout = StreamLayout::new(vec![StreamDescriptor::new(
                    *codec, *width, *height, *fps, 8,
                )]);
                knowledge_store.record_telemetry(CodecCorpusEntry {
                    fingerprint,
                    capability_bundle: mediamaster_core::caps::CapsMatrix::default(),
                    observed_layout: layout,
                    telemetry: StarvationTelemetry {
                        dropped_frames_per_sec: 10.0,
                        jitter_ms: 20.0,
                        decoder_starvation_events: acc.raw_starvations,
                        surface_allocation_failures: 0,
                    },
                    recommended_degradations: Vec::new(),
                });
            }
            HardwareEvent::SlotRevoked {
                device_class,
                codec,
                reason: _,
            } => {
                let key = (device_class.to_lowercase(), *codec);
                let acc = match self.observations.get_mut(&key) {
                    Some(existing) => {
                        existing.decay(now_ms, self.half_life_ms);
                        existing
                    }
                    None => {
                        self.observations.insert(
                            key.clone(),
                            ObservationAccumulator::new(device_class.clone(), *codec, now_ms),
                        );
                        self.observations.get_mut(&key).unwrap()
                    }
                };

                acc.raw_starvations += 2;
                acc.weighted_starvations += 2.0;

                let obs = acc.to_observed_caps(self.starvation_threshold);
                corpus.record_observation(&obs);
            }
            HardwareEvent::DecodeHeartbeat {
                device_class,
                codec,
                width,
                height,
                fps,
                concurrent_streams,
                duration_ms,
            } => {
                let key = (device_class.to_lowercase(), *codec);
                let acc = match self.observations.get_mut(&key) {
                    Some(existing) => {
                        existing.decay(now_ms, self.half_life_ms);
                        existing
                    }
                    None => {
                        self.observations.insert(
                            key.clone(),
                            ObservationAccumulator::new(device_class.clone(), *codec, now_ms),
                        );
                        self.observations.get_mut(&key).unwrap()
                    }
                };

                acc.weighted_heartbeat_ms += *duration_ms as f64;
                acc.max_width_observed = acc.max_width_observed.max(*width);
                acc.max_height_observed = acc.max_height_observed.max(*height);
                acc.max_fps_observed = acc.max_fps_observed.max(*fps);
                acc.max_concurrent_streams = acc.max_concurrent_streams.max(*concurrent_streams);

                let obs = acc.to_observed_caps(self.starvation_threshold);
                corpus.record_observation(&obs);
            }
            _ => {}
        }
    }

    /// Look up current observed capability summary for a `device_class` and `codec`.
    pub fn get_observed_caps(&self, device_class: &str, codec: Codec) -> Option<ObservedCodecCaps> {
        let key = (device_class.to_lowercase(), codec);
        self.observations
            .get(&key)
            .map(|acc| acc.to_observed_caps(self.starvation_threshold))
    }
}

impl Default for TelemetryIngest {
    fn default() -> Self {
        Self::new()
    }
}
