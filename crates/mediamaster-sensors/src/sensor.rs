//! Async sensor trait definition.
//!
//! Provides the canonical `AsyncSensor` trait for hardware backends and mock/simulated streams.

use std::boxed::Box;
use std::future::Future;
use std::pin::Pin;

use mediamaster_core::traits::SensorReading;
use mediamaster_core::MediamasterError;

/// A hardware sensor read through an object-safe async interface.
///
/// Returns a boxed future so `Box<dyn AsyncSensor>` is valid. `&mut self` so a
/// driver may keep per-poll state without interior mutability.
pub trait AsyncSensor: Send + Sync {
    /// Poll the next sensor sample asynchronously.
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummySensor;
    impl AsyncSensor for DummySensor {
        fn read_sample<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>>
        {
            Box::pin(async { Ok(SensorReading::Occupancy) })
        }
    }

    #[tokio::test]
    async fn test_object_safety() {
        let mut sensor: Box<dyn AsyncSensor> = Box::new(DummySensor);
        let sample = sensor.read_sample().await.unwrap();
        assert_eq!(sample, SensorReading::Occupancy);
    }
}
