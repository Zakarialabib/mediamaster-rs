//! Shared types for the mediamaster-rs ecosystem.

use serde::{Deserialize, Serialize};

/// An aggregate room state derived from fused sensor readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomState {
    Empty,
    Occupied,
    LowLight,
    MotionDetected,
    PersonPresent,
    Unavailable,
}

/// A detected RF burst after protocol parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioBurst {
    pub id: String,
    pub frequency_hz: u32,
    pub power_db: f32,
    pub payload: Vec<u8>,
    pub detected_by: String,
}

/// An axis-aligned rectangle, used for CCTV motion bounding boxes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A target for audio output on a playback device (from core, shared by audio
/// routing and signage adapters).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTarget {
    pub label: String,
    pub device: Option<String>,
    pub priority: u8,
}

impl BoundingBox {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px <= self.x + self.width && py <= self.y + self.height
    }
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
}
