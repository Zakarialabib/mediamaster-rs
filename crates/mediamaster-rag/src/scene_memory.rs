//! Environmental & Acoustic Scene Memory.
//!
//! Historical patterns from FusionEngine (RoomState: PIR motion + mmWave +
//! ambient lux + RF 433MHz triggers) correlated with AudioZone volume/ducking levels.
//!
//! Replaces rigid threshold rules (e.g., `if lux < 50`) with historical
//! nearest-neighbor clustering, returning smoothed transition profiles
//! (e.g., 5-second linear crossfades and adjusted ducking sensitivities).

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::error::RagError;

/// Continuous normalized sensor vector capturing the environmental acoustic scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorVector {
    /// Motion activity index in [0.0, 1.0] (e.g. PIR pulses normalized over window).
    pub motion_density: f32,
    /// Proximity or mmWave distance in meters.
    pub mmwave_distance_m: f32,
    /// Ambient illumination in lux.
    pub ambient_lux: f32,
    /// Rate of change of illumination in lux/sec.
    pub lux_rate_of_change: f32,
    /// ISM radio burst frequency in bursts/minute (e.g., 433/868 MHz clickers/sensors).
    pub rf_burst_frequency: f32,
}

impl SensorVector {
    pub fn new(
        motion_density: f32,
        mmwave_distance_m: f32,
        ambient_lux: f32,
        lux_rate_of_change: f32,
        rf_burst_frequency: f32,
    ) -> Self {
        Self {
            motion_density,
            mmwave_distance_m,
            ambient_lux,
            lux_rate_of_change,
            rf_burst_frequency,
        }
    }

    /// Normalized weighted Euclidean distance between two sensor states.
    pub fn distance(&self, other: &SensorVector) -> f32 {
        let d_motion = (self.motion_density - other.motion_density).powi(2) * 4.0;
        let d_dist = ((self.mmwave_distance_m - other.mmwave_distance_m) / 10.0).powi(2) * 1.5;
        let d_lux = ((self.ambient_lux - other.ambient_lux) / 500.0).powi(2) * 2.0;
        let d_lux_rate = ((self.lux_rate_of_change - other.lux_rate_of_change) / 50.0).powi(2);
        let d_rf = ((self.rf_burst_frequency - other.rf_burst_frequency) / 60.0).powi(2) * 1.5;

        (d_motion + d_dist + d_lux + d_lux_rate + d_rf).sqrt()
    }
}

/// Contextual temporal metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalMetadata {
    /// Day of week (0 = Sunday, 6 = Saturday).
    pub day_of_week: u8,
    /// Hour of day (0..23).
    pub hour_of_day: u8,
    pub is_weekend: bool,
    pub season_tag: String,
}

impl TemporalMetadata {
    pub fn new(
        day_of_week: u8,
        hour_of_day: u8,
        is_weekend: bool,
        season_tag: impl Into<String>,
    ) -> Self {
        Self {
            day_of_week,
            hour_of_day,
            is_weekend,
            season_tag: season_tag.into(),
        }
    }

    /// Distance metric for time-of-day cyclicity.
    pub fn time_distance(&self, other: &TemporalMetadata) -> f32 {
        let hour_diff = (self.hour_of_day as i32 - other.hour_of_day as i32).abs();
        let cyclic_diff = hour_diff.min(24 - hour_diff) as f32;
        let weekend_penalty = if self.is_weekend == other.is_weekend {
            0.0
        } else {
            2.0
        };
        cyclic_diff + weekend_penalty
    }
}

/// Optimal audio configuration learned or designated for a scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimalAudioState {
    /// Master volume in dB (e.g. 0.0dB, -6.0dB, -18.0dB).
    pub master_volume_db: f32,
    /// Suggested ducking target gain (0.0 = full mute, 0.125 = -18dB).
    pub ducking_target_gain: f32,
    /// Ducking release/attack sensitivity in ms.
    pub ducking_hold_ms: u64,
    /// Equalizer / acoustic profile name (e.g. "WarmRetail", "NightAmbient", "HighCrowd").
    pub acoustic_preset: String,
    /// Target crossfade duration in milliseconds to transition smoothly.
    pub crossfade_duration_ms: u64,
}

/// Full atmosphere playbook retrieved for a scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtmospherePlaybook {
    pub scene_label: String,
    pub audio_state: OptimalAudioState,
    pub match_confidence: f32,
    pub description: String,
}

/// A stored scene observation in memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneRecord {
    pub sensor_vector: SensorVector,
    pub temporal: TemporalMetadata,
    pub playbook: AtmospherePlaybook,
}

/// Query to match current sensor telemetry to an atmosphere playbook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneQuery {
    pub current_sensors: SensorVector,
    pub current_temporal: TemporalMetadata,
}

/// Bounded memory store for environmental and acoustic scene patterns.
#[derive(Debug, Clone)]
pub struct SceneMemoryStore {
    capacity: usize,
    records: VecDeque<SceneRecord>,
}

impl SceneMemoryStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            records: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Store a scene record, evicting the oldest record if bounded capacity is reached.
    pub fn record_scene(&mut self, record: SceneRecord) {
        if self.records.len() >= self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    /// Query the scene memory using nearest-neighbor clustering over normalized sensor
    /// vector and temporal distance.
    pub fn query_scene_memory(&self, query: &SceneQuery) -> Result<AtmospherePlaybook, RagError> {
        if self.records.is_empty() {
            return Err(RagError::EmptyStore);
        }

        let mut best_record = None;
        let mut min_distance = f32::INFINITY;

        for record in &self.records {
            let sensor_dist = query.current_sensors.distance(&record.sensor_vector);
            let time_dist = query.current_temporal.time_distance(&record.temporal) * 0.1;
            let total_dist = sensor_dist + time_dist;

            if total_dist < min_distance {
                min_distance = total_dist;
                best_record = Some(record);
            }
        }

        match best_record {
            Some(record) => {
                // Confidence falls off with distance: 1.0 / (1.0 + distance)
                let confidence = (1.0 / (1.0 + min_distance)).clamp(0.0, 1.0);
                let mut playbook = record.playbook.clone();
                playbook.match_confidence = confidence;
                Ok(playbook)
            }
            None => Err(RagError::NoMatchFound),
        }
    }
}
