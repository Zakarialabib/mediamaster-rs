//! Fusion engine — combines aggregated sensor readings into a high-level
//! [`RoomState`].

use std::collections::HashMap;

use mediamaster_core::traits::SensorReading;
use mediamaster_core::types::RoomState;

/// Produces a fused `RoomState` from the latest reading of every sensor.
///
/// Fusion policy (priority-ordered, deterministic regardless of HashMap
/// iteration order):
///
/// 1. **Motion + low light** → [`RoomState::PersonPresent`]
///    (a person is detected in a dim area).
/// 2. **Motion / occupancy** → [`RoomState::Occupied`].
/// 3. **Proximity < 10 m** → [`RoomState::PersonPresent`].
/// 4. **Light < 50 lux** → [`RoomState::LowLight`].
/// 5. Otherwise → [`RoomState::Empty`].
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

    /// Compute the current fused `RoomState`.
    pub fn current_state(&self) -> RoomState {
        let has_motion = self
            .readings
            .values()
            .any(|r| matches!(r, SensorReading::MotionDetected | SensorReading::Occupancy));
        let low_light = self
            .readings
            .values()
            .any(|r| matches!(r, SensorReading::Light(l) if *l < 50.0));

        // 1 — motion + low light ⇒ person present (more specific than Occupied)
        if has_motion && low_light {
            return RoomState::PersonPresent;
        }

        // 2 — motion/occupancy only ⇒ occupied
        if has_motion {
            return RoomState::Occupied;
        }

        // 3 — proximity nearby ⇒ person present
        for reading in self.readings.values() {
            if let SensorReading::Proximity(v) = reading {
                if *v < 10.0 {
                    return RoomState::PersonPresent;
                }
            }
        }

        // 4 — low light
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
    fn motion_plus_low_light_is_person_present() {
        let mut engine = FusionEngine::new();
        engine.update("light", SensorReading::Light(10.0));
        engine.update("pir", SensorReading::MotionDetected);
        assert_eq!(engine.current_state(), RoomState::PersonPresent);
    }

    #[test]
    fn motion_without_light_is_occupied() {
        let mut engine = FusionEngine::new();
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

    #[test]
    fn proximity_near_person() {
        let mut engine = FusionEngine::new();
        engine.update("tof", SensorReading::Proximity(0.5));
        assert_eq!(engine.current_state(), RoomState::PersonPresent);
    }

    #[test]
    fn bright_room_is_empty() {
        let mut engine = FusionEngine::new();
        engine.update("light", SensorReading::Light(500.0));
        assert_eq!(engine.current_state(), RoomState::Empty);
    }
}
