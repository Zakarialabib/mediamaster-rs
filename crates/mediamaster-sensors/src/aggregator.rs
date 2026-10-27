//! Context state aggregation — collects the latest reading from each named
//! sensor into a unified map that the [`FusionEngine`](crate::fusion::FusionEngine)
//! consumes.

use std::collections::HashMap;

use mediamaster_core::traits::SensorReading;

/// Holds the most recent reading from every registered sensor.
#[derive(Default)]
pub struct SensorAggregator {
    readings: HashMap<String, SensorReading>,
}

impl SensorAggregator {
    pub fn new() -> Self {
        Self {
            readings: HashMap::new(),
        }
    }

    /// Record (or replace) the latest reading for `sensor_id`.
    pub fn push(&mut self, sensor_id: impl Into<String>, reading: SensorReading) {
        self.readings.insert(sensor_id.into(), reading);
    }

    /// Read-only access to all stored readings.
    pub fn readings(&self) -> &HashMap<String, SensorReading> {
        &self.readings
    }

    /// Number of sensors currently tracked.
    pub fn len(&self) -> usize {
        self.readings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.readings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_latest_per_sensor() {
        let mut agg = SensorAggregator::new();
        agg.push("pir-1", SensorReading::MotionDetected);
        agg.push("pir-1", SensorReading::Occupancy);
        agg.push("dht-1", SensorReading::Light(42.0));

        assert_eq!(agg.len(), 2);
        assert_eq!(agg.readings().get("pir-1"), Some(&SensorReading::Occupancy));
    }
}
