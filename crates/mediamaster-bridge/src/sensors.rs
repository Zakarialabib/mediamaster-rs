//! Sensors domain bridge: adapter and device enumerators.
//!
//! This module bridges the `mediamaster-core::AsyncSensor` trait to the
//! `mediamaster-sensors` domain crate, and provides device enumerators for
//! internal and external sensor hardware.

use std::future::Future;
use std::pin::Pin;

use mediamaster_core::types::SensorReading;
use mediamaster_core::MediamasterError;
use mediamaster_sensors::fusion::FusionEngine;
use mediamaster_sensors::AsyncSensor;

/// Bridge from `mediamaster-sensors::AsyncSensor` → `mediamaster-sensors`.
///
/// `SensorBridge` wraps a `FusionEngine` and provides the `AsyncSensor` trait
/// implementation for the applications.
pub struct SensorBridge {
    fusion: FusionEngine,
}

impl SensorBridge {
    /// Create a new sensor bridge.
    pub fn new() -> Self {
        Self {
            fusion: FusionEngine::new(),
        }
    }

    /// Enumerate internal sensor devices.
    ///
    /// Internal sensors are device-embedded (e.g., camera-based motion,
    /// device temperature sensor).
    pub fn enumerate_internal_sensor_devices(&self) -> Vec<String> {
        // Placeholder: applications query hardware APIs.
        vec![
            "camera-based motion".to_string(),
            "device temperature".to_string(),
        ]
    }

    /// Enumerate external sensor devices.
    ///
    /// External sensors include:
    /// - USB sensors
    /// - BLE sensors
    pub fn enumerate_external_sensor_devices(&self) -> Vec<String> {
        vec!["USB sensor".to_string(), "BLE sensor".to_string()]
    }

    /// Get the fusion engine (for direct access by the application).
    pub fn fusion(&self) -> &FusionEngine {
        &self.fusion
    }

    /// Get the fusion engine (mutable).
    pub fn fusion_mut(&mut self) -> &mut FusionEngine {
        &mut self.fusion
    }
}

impl Default for SensorBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncSensor for SensorBridge {
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>> {
        Box::pin(async move {
            // Placeholder: read from a sensor, then update fusion.
            Ok(SensorReading::Unknown)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_devices() {
        let bridge = SensorBridge::new();
        assert!(!bridge.enumerate_internal_sensor_devices().is_empty());
        assert!(!bridge.enumerate_external_sensor_devices().is_empty());
        let _ = bridge.fusion();
        let _ = SensorBridge::default();
    }
}
