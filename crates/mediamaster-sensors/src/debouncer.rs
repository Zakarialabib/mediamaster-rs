//! Time-based debouncing for raw sensor readings.

use std::time::{Duration, Instant};

use mediamaster_core::traits::SensorReading;

/// Wraps a sensor reading stream and only emits a reading when it has been
/// **stable** for the configured `window`.
///
/// Transient noise (a PIR blip, a light flicker) is filtered out: the internal
/// state only transitions once the same reading persists across `window`.
pub struct DebouncedSensor {
    window: Duration,
    last_reading: Option<SensorReading>,
    last_change: Option<Instant>,
}

impl DebouncedSensor {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_reading: None,
            last_change: None,
        }
    }

    /// Feed a raw reading. Returns `Some(reading)` only when the reading has
    /// been stable for `window` since the last transition.
    pub fn update(&mut self, reading: SensorReading) -> Option<SensorReading> {
        let now = Instant::now();
        match (&self.last_reading, &self.last_change) {
            (Some(prev), Some(since)) if *prev == reading => {
                if now.duration_since(*since) >= self.window {
                    Some(reading)
                } else {
                    None
                }
            }
            _ => {
                self.last_reading = Some(reading);
                self.last_change = Some(now);
                None
            }
        }
    }

    pub fn window(&self) -> Duration {
        self.window
    }
}

impl Default for DebouncedSensor {
    fn default() -> Self {
        Self::new(Duration::from_millis(50))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_only_after_stable_window() {
        let mut db = DebouncedSensor::new(Duration::from_millis(100));
        // First reading starts the window.
        assert!(db.update(SensorReading::MotionDetected).is_none());
        // Same reading again but within window → still None.
        assert!(db.update(SensorReading::MotionDetected).is_none());
        // Different reading resets the window.
        assert!(db.update(SensorReading::Open).is_none());
        // Back to MotionDetected restarts again.
        assert!(db.update(SensorReading::MotionDetected).is_none());
    }

    #[test]
    fn emits_immediately_with_zero_window() {
        let mut db = DebouncedSensor::new(Duration::from_millis(0));
        assert!(db.update(SensorReading::Open).is_none()); // first call: starts window
                                                           // Same reading, window already elapsed → emits
        assert_eq!(db.update(SensorReading::Open), Some(SensorReading::Open));
    }
}
