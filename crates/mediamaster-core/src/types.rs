//! Shared types for the mediamaster-rs ecosystem.
//!
//! All public types are re-exported from this module via [`crate`] so
//! downstream users write `mediamaster_core::RoomState`, not
//! `mediamaster_core::types::RoomState`.

use serde::{Deserialize, Serialize};

/// An aggregate room state derived from fused sensor readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomState {
    /// No detection in the last poll window.
    Empty,
    /// At least one sensor reports presence.
    Occupied,
    /// Ambient light is below threshold.
    LowLight,
    /// PIR/motion sensor triggered.
    MotionDetected,
    /// Occupancy + active human presence (light + motion together).
    PersonPresent,
    /// Sensor subsystem is unreachable or in an error state.
    Unavailable,
}

/// A detected RF burst after protocol parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioBurst {
    pub id: String,
    pub frequency_hz: u32,
    /// Instantaneous power in dB relative to the detector's reference.
    pub power_db: f32,
    pub payload: Vec<u8>,
    /// Which detector/parser produced this burst (e.g. `"OOK@433MHz"`).
    pub detected_by: String,
}

/// An axis-aligned rectangle, used for CCTV motion bounding boxes.
///
/// Does **not** derive `Eq` because `f32` fields cannot satisfy the
/// total-equality contract (NaN ≠ NaN).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BoundingBox {
    /// Check whether a pixel coordinate falls inside (or on the boundary of)
    /// this bounding box.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px <= self.x + self.width && py <= self.y + self.height
    }

    /// Area in pixels.
    pub fn area(&self) -> f32 {
        self.width * self.height
    }
}

/// A target for audio output on a playback device (from core, shared by audio
/// routing and signage adapters).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTarget {
    pub label: String,
    pub device: Option<String>,
    pub priority: u8,
}

/// One debounced sensor reading.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SensorReading {
    /// Motion detected (PIR, mmWave, camera-based).
    MotionDetected,
    /// Proximity in metres (0.0 = touching, `f32::INFINITY` = unknown).
    Proximity(f32),
    /// Ambient light level in lux.
    Light(f32),
    /// Binary occupancy (e.g. seat sensor).
    Occupancy,
    /// Binary contact closure (door/window sensor).
    Open,
    Closed,
    /// Sensor returned an invalid or unreadable value.
    Unknown,
}

impl SensorReading {
    /// Relative fusion priority: higher = more influential on `RoomState`.
    ///
    /// `FusionEngine` uses this to break ties when multiple readings arrive
    /// concurrently.
    pub fn priority(&self) -> u8 {
        match self {
            Self::MotionDetected => 5,
            Self::Occupancy => 4,
            Self::Proximity(_) => 3,
            Self::Open | Self::Closed => 2,
            Self::Light(_) => 1,
            Self::Unknown => 0,
        }
    }
}

/// A detected RF signal — whatever a user's `SignalDetector` decides it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    /// Human-readable identifier (e.g. bin index, protocol name).
    pub id: String,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f32,
    /// Starting sample index in the original IQ stream.
    pub start_sample: usize,
    /// Peak magnitude observed.
    pub magnitude_peak: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_interior_and_boundary() {
        let box_ = BoundingBox {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(box_.contains(5.0, 5.0));
        assert!(box_.contains(0.0, 0.0));
        assert!(box_.contains(10.0, 10.0));
        assert!(!box_.contains(-1.0, 5.0));
        assert!(!box_.contains(5.0, 11.0));
        assert!(!box_.contains(11.0, 5.0));
    }

    #[test]
    fn area_is_width_times_height() {
        let bb = BoundingBox {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 3.0,
        };
        assert!((bb.area() - 12.0).abs() < 1e-6);
    }

    #[test]
    fn sensor_reading_priority_ordering() {
        let readings = [
            SensorReading::Unknown,
            SensorReading::Light(50.0),
            SensorReading::Open,
            SensorReading::Proximity(0.5),
            SensorReading::Occupancy,
            SensorReading::MotionDetected,
        ];
        let priorities: Vec<u8> = readings.iter().map(|r| r.priority()).collect();
        assert_eq!(priorities, vec![0, 1, 2, 3, 4, 5]);
    }
}
