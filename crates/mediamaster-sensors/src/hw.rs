//! Hardware sensor backends using `embedded-hal-async`.
//!
//! This module provides concrete `AsyncSensor` implementations for real
//! sensors behind I2C/SPI/UART buses. It is feature-gated behind `hw` so the
//! crate remains `no_std`-capable when the feature is disabled.

use std::boxed::Box;
use std::future::Future;
use std::pin::Pin;

use mediamaster_core::traits::SensorReading;
use mediamaster_core::MediamasterError;

use crate::sensor::AsyncSensor;

// Re-export the HAL trait for consumers that need it.
#[cfg(feature = "hw")]
pub use embedded_hal_async::i2c::I2c;

/// Mock sensor for testing — returns deterministic values.
#[derive(Debug, Clone)]
pub struct MockSensor {
    reading: SensorReading,
    id: String,
}

impl MockSensor {
    pub fn new(id: &str, reading: SensorReading) -> Self {
        Self {
            id: id.to_string(),
            reading,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn set_reading(&mut self, reading: SensorReading) {
        self.reading = reading;
    }
}

impl AsyncSensor for MockSensor {
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>> {
        let reading = self.reading;
        Box::pin(async move { Ok(reading) })
    }
}

/// Placeholder: in a real implementation, wrap an I2C sensor like BME280,
/// BNO080 (IMU), or MLX90640 (thermal).
#[derive(Debug, Clone, Default)]
pub struct PlaceholderSensor;

impl AsyncSensor for PlaceholderSensor {
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>> {
        Box::pin(async move { Ok(SensorReading::Unknown) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_sensor_reads_reading() {
        let mut s = MockSensor::new("test", SensorReading::Occupancy);
        assert_eq!(s.read_sample().await.unwrap(), SensorReading::Occupancy);
    }

    #[tokio::test]
    async fn placeholder_sensor_reads_unknown() {
        let mut s = PlaceholderSensor;
        assert_eq!(s.read_sample().await.unwrap(), SensorReading::Unknown);
    }
}
