//! Hardware event vocabulary + the runtime-agnostic event sink trait.

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

use crate::types::{BoundingBox, RadioBurst, RoomState};

/// Every event the ecosystem publishes and consumers subscribe to.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum HardwareEvent {
    /// High-priority event requested a volume duck on a named audio source.
    AudioDuckRequest {
        source: String,
        ducking: f32,
        until: u64,
    },
    /// A debounced sensor fusion state changed.
    SensorContextChange { state: RoomState },
    /// An RF burst was detected in the spectrum.
    RadioBurstDetected { burst: RadioBurst },
    /// A CCTV camera reported motion.
    CctvMotionDetected {
        camera: String,
        bounding_box: BoundingBox,
    },
    /// An audio zone's volume or mute state changed.
    ZoneChanged {
        zone_ids: Vec<String>,
        volume_db: f32,
        muted: bool,
    },
    /// Continuous environmental sensor drift telemetry for atmospheric harmonization.
    EnvironmentalDrift {
        motion_density: f32,
        mmwave_distance_m: f32,
        ambient_lux: f32,
        lux_rate_of_change: f32,
        rf_burst_frequency: f32,
    },
    /// Audio stream buffer underrun detected on a source.
    AudioUnderrun {
        source: String,
        dropped_frames: usize,
    },
    /// Audio stream buffer overrun detected on a source.
    AudioOverrun {
        source: String,
        dropped_frames: usize,
    },
}

impl HardwareEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            HardwareEvent::AudioDuckRequest { .. } => "AudioDuckRequest",
            HardwareEvent::SensorContextChange { .. } => "SensorContextChange",
            HardwareEvent::RadioBurstDetected { .. } => "RadioBurstDetected",
            HardwareEvent::CctvMotionDetected { .. } => "CctvMotionDetected",
            HardwareEvent::ZoneChanged { .. } => "ZoneChanged",
            HardwareEvent::EnvironmentalDrift { .. } => "EnvironmentalDrift",
            HardwareEvent::AudioUnderrun { .. } => "AudioUnderrun",
            HardwareEvent::AudioOverrun { .. } => "AudioOverrun",
        }
    }
}

/// Synchronous, runtime-agnostic event sink injected by the host runtime
/// (`mediamaster-bus`, `mediamaster-agent`, or an application loop).
///
/// `mediamaster-core` never owns a concrete channel: the host supplies any
/// implementation (tokio broadcast, crossbeam, a ring buffer, a test double).
pub trait EventPublisher: Send + Sync {
    /// Publishes a hardware or pipeline state transition event.
    fn publish(&self, event: HardwareEvent);
}
