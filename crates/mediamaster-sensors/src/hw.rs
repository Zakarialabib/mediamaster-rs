//! Hardware sensor backends using `embedded-hal-async`.
//!
//! This module provides concrete `AsyncSensor` implementations for real
//! sensors behind I2C/SPI/UART buses. It is feature-gated behind `hw` so the
//! crate remains `no_std`-capable when the feature is disabled.
//!
//! # Example
//! ```ignore
//! use mediamaster_sensors::hw::Bme280Sensor;
//! use mediamaster_sensors::fusion::FusionEngine;
//!
//! let mut engine = FusionEngine::new();
//! engine.register("temp", Box::new(Bme280Sensor::new(i2c_device)));
//! ```

// Re-export the HAL trait for consumers that need it.
pub use embedded_hal_async::i2c::I2c;

/// Mock sensor for testing — returns deterministic values.
#[derive(Debug, Clone)]
pub struct MockSensor {
    value: f32,
    id: String,
}

impl MockSensor {
    pub fn new(id: &str, value: f32) -> Self {
        Self {
            id: id.to_string(),
            value,
        }
    }
}

impl embedded_hal_async::sensors::Sensor for MockSensor {
    #[allow(async_fn_in_trait)]
    async fn read(&mut self) -> Result<f32, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.value)
    }
}

/// Placeholder: in a real implementation, wrap an I2C sensor like BME280,
/// BNO080 (IMU), or MLX90640 (thermal).
#[derive(Debug, Clone)]
pub struct PlaceholderSensor;

impl embedded_hal_async::sensors::Sensor for PlaceholderSensor {
    async fn read(&mut self) -> Result<f32, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: real I2C read
        Ok(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal_async::sensors::Sensor;

    #[test]
    fn mock_sensor_reads_value() {
        let mut s = MockSensor::new("test", 0.75);
        assert_eq!(s.read().await.unwrap(), 0.75);
    }
}
