//! Fusion engine — combines aggregated sensor readings into a high-level
//! [`RoomState`].

use std::collections::HashMap;

use mediamaster_core::traits::SensorReading;
use mediamaster_core::types::RoomState;

/// Produces a fused `RoomState` from the latest reading of every sensor.
///
/// The fusion policy is intentionally simple (priority-ordered scan): any
/// motion/occupancy reading ⇒ `Occupied`; a nearby proximity reading ⇒
/// `PersonPresent`; low light ⇒ `LowLight`; otherwise `Empty`.
pub struct FusionEngine {
    readings: HashMap<String, SensorReading>,
}

impl FusionEngine {
    pub fn new() -> Self {
        Self {
            readings: HashMap::new(),
        }
    }

    /// Update the latest reading for a sensor.
    pub fn update(&mut self, sensor_id: impl Into<String>, reading: SensorReading) {
        self.readings.insert(sensor_id.into(), reading);
    }

    /// Compute the current fused `RoomState` using priority-ordered checks
    /// so that iteration order of the backing map does not affect the result.
    pub fn current_state(&self) -> RoomState {
        // Priority 1: motion/occupancy trumps everything.
        for reading in self.readings.values() {
            if matches!(reading, SensorReading::MotionDetected | SensorReading::Occupancy) {
                return RoomState::Occupied;
            }
        }
        // Priority 2: proximity (person nearby).
        for reading in self.readings.values() {
            if let SensorReading::Proximity(v) = reading {
                if *v < 10.0 {
                    return RoomState::PersonPresent;
                }
            }
        }
        // Priority 3: low light.
        for reading in self.readings.values() {
            if let SensorReading::Light(v) = reading {
                if *v < 50.0 {
                    return RoomState::LowLight;
                }
            }
        }
        RoomState::Empty
    }
}

impl Default for FusionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_trumps_other_states() {
        let mut engine = FusionEngine::new();
        engine.update("light", SensorReading::Light(10.0));
        engine.update("pir", SensorReading::MotionDetected);
        assert_eq!(engine.current_state(), RoomState::Occupied);
    }

    #[test]
    fn empty_when_no_readings() {
        let engine = FusionEngine::new();
        assert_eq!(engine.current_state(), RoomState::Empty);
    }

    #[test]
    fn low_light_without_motion() {
        let mut engine = FusionEngine::new();
        engine.update("light", SensorReading::Light(10.0));
        assert_eq!(engine.current_state(), RoomState::LowLight);
    }
}
