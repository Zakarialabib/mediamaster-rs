//! Deterministic simulated sensor stream for testing.

use std::boxed::Box;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use mediamaster_core::traits::SensorReading;
use mediamaster_core::MediamasterError;

use crate::sensor::AsyncSensor;

/// A scripted sensor reading with an optional delay offset.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptedReading {
    pub delay: Duration,
    pub reading: SensorReading,
}

impl ScriptedReading {
    pub fn new(delay: Duration, reading: SensorReading) -> Self {
        Self { delay, reading }
    }

    pub fn immediate(reading: SensorReading) -> Self {
        Self {
            delay: Duration::ZERO,
            reading,
        }
    }
}

/// A simulated sensor stream that returns scripted timestamped readings.
pub struct SimulatedSensorStream {
    readings: Vec<ScriptedReading>,
    cursor: usize,
}

impl SimulatedSensorStream {
    pub fn new(readings: Vec<ScriptedReading>) -> Self {
        Self {
            readings,
            cursor: 0,
        }
    }

    pub fn from_readings(readings: impl IntoIterator<Item = SensorReading>) -> Self {
        let scripted = readings
            .into_iter()
            .map(ScriptedReading::immediate)
            .collect();
        Self::new(scripted)
    }
}

impl AsyncSensor for SimulatedSensorStream {
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>> {
        Box::pin(async move {
            if self.cursor >= self.readings.len() {
                return Ok(SensorReading::Unknown);
            }
            let item = &self.readings[self.cursor];
            self.cursor += 1;
            if !item.delay.is_zero() {
                tokio::time::sleep(item.delay).await;
            }
            Ok(item.reading)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stream_yields_scripted_readings() {
        let readings = vec![
            ScriptedReading::new(Duration::from_millis(10), SensorReading::Occupancy),
            ScriptedReading::immediate(SensorReading::MotionDetected),
        ];
        let mut stream = SimulatedSensorStream::new(readings);
        assert_eq!(
            stream.read_sample().await.unwrap(),
            SensorReading::Occupancy
        );
        assert_eq!(
            stream.read_sample().await.unwrap(),
            SensorReading::MotionDetected
        );
        assert_eq!(stream.read_sample().await.unwrap(), SensorReading::Unknown);
    }
}
