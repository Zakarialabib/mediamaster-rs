//! Three-stage async pipeline over an I/Q source.
//!
//! - **Acquisition** — reads raw I/Q samples from the SDR backend and
//!   forwards batches to the DSP stage.
//! - **DSP** — applies an FFT and detects peak magnitudes with a
//!   `MagnitudeDetector`.
//! - **Protocol parsing** — evaluates a `ProtocolParser` (built on `nom`)
//!   over the sample stream, recognizes protocol headers and publishes
//!   `mediamaster_core::events::RadioBurstDetected` events onto the
//!   `EventBus`.
//!
//! ## Notes for implementors
//!
//! - The pipeline owns no runtime: it must be driven from a tokio runtime
//!   (`#[tokio::main]` or `Runtime::block_on`). This keeps `mediamaster-rf`
//!   a pure DSP crate beside the runtime.
//! - The byte conversion in stage 3 is a **placeholder for the demodulator**.
//!   Payload bytes are transported in the real part of the I/Q samples
//!   (one byte per sample, values 0–255). Real front-ends (RTL-SDR, SoapySDR)
//!   feed the payload bytes from their native demodulation path; nothing in
//!   this crate needs to change — only the byte extraction step differs.
//! - The pipeline runs for as long as the source produces data. The source
//!   signals end-of-stream by returning an empty batch; the pipeline then
//!   joins all stages and returns `Ok(())`.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use num_complex::Complex;
//! use mediamaster_core::events::EventBus;
//! use mediamaster_core::MediamasterError;
//! use mediamaster_rf::{Pipeline, PipelineConfig, ProtocolParser};
//!
//! # async fn example() -> Result<(), MediamasterError> {
//! let bus = Arc::new(EventBus::new(16));
//! let config = PipelineConfig {
//!     batch_size: 1024,
//!     fft_size: 1024,
//!     threshold: 0.0,
//!     buffer_size: 8,
//!     center_frequency_hz: 433_920_000,
//! };
//! struct FakeSource;
//! impl mediamaster_rf::IqSource for FakeSource {
//!     async fn acquire(&self, _count: usize) -> Vec<Complex<f32>> {
//!         unimplemented!()
//!     }
//!     fn sample_rate_hz(&self) -> u32 {
//!         1000
//!     }
//! }
//! let pipeline = Pipeline::new(
//!     Arc::new(FakeSource),
//!     Arc::new(()),
//!     bus,
//!     config,
//! );
//! pipeline.run().await?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;

use crate::iq_source::{IqSample, IqSource};
use mediamaster_core::events::{EventBus, HardwareEvent};
use mediamaster_core::MediamasterError;
use mediamaster_core::RadioBurst;
use mediamaster_core::SignalDetector;

use crate::detector::MagnitudeDetector;
use crate::parser::ProtocolParser;

/// Configuration for a [`Pipeline`].
///
/// - `batch_size`: number of I/Q samples fetched from the source per
///   acquisition round.
/// - `fft_size`: FFT size of the DSP stage (must match the batch length of
///   the source in practice).
/// - `threshold`: peak-detection threshold; only magnitudes strictly above
///   this count as signals.
/// - `buffer_size`: capacity of the internal `mpsc` channels (tunes
///   backpressure between stages).
/// - `center_frequency_hz`: RF carrier centre frequency attached to every
///   emitted `RadioBurst` (for traceability).
pub struct PipelineConfig {
    pub batch_size: usize,
    pub fft_size: usize,
    pub threshold: f32,
    pub buffer_size: usize,
    pub center_frequency_hz: u32,
}

/// Stage 1 — IQ acquisition.
///
/// Runs the source's `acquire` loop and forwards each batch on the
/// `mpsc` channel to the downstream DSP stage.
pub struct AcquisitionStage<S> {
    source: Arc<S>,
    batch_size: usize,
}

impl<S: IqSource> AcquisitionStage<S> {
    /// Build the acquisition stage over `source`, fetching `batch_size`
    /// samples per round.
    pub fn new(source: Arc<S>, batch_size: usize) -> Self {
        Self { source, batch_size }
    }

    /// Drive the acquisition loop. Each non-empty batch is sent on `tx`
    /// until the source returns an empty batch (end of stream).
    pub async fn run(
        self,
        tx: tokio::sync::mpsc::Sender<Vec<IqSample>>,
    ) -> Result<(), MediamasterError> {
        loop {
            let batch = self.source.acquire(self.batch_size).await;
            if batch.is_empty() {
                break Ok(());
            }
            tx.send(batch)
                .await
                .map_err(|_| MediamasterError::Other("pipeline halted".to_string()))?;
        }
    }
}

/// Three-stage async pipeline over an I/Q source.
///
/// The pipeline is fully generic over the source type `S` (the concrete
/// `IqSource` implementation), so the same pipeline runs over RTL-SDR,
/// SoapySDR, a test mock, or any other `IqSource`.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use num_complex::Complex;
/// use mediamaster_core::events::EventBus;
/// use mediamaster_core::MediamasterError;
/// use mediamaster_rf::{Pipeline, PipelineConfig, ProtocolParser};
///
/// # async fn example() -> Result<(), MediamasterError> {
/// let bus = Arc::new(EventBus::new(16));
/// let config = PipelineConfig {
///     batch_size: 1024,
///     fft_size: 1024,
///     threshold: 0.0,
///     buffer_size: 8,
///     center_frequency_hz: 433_920_000,
/// };
/// struct FakeSource;
/// impl mediamaster_rf::IqSource for FakeSource {
///     async fn acquire(&self, _count: usize) -> Vec<Complex<f32>> {
///         unimplemented!()
///     }
///     fn sample_rate_hz(&self) -> u32 {
///         1000
///     }
/// }
/// let pipeline = Pipeline::new(
///     Arc::new(FakeSource),
///     Arc::new(()),
///     bus,
///     config,
/// );
/// pipeline.run().await?;
/// # Ok(())
/// # }
/// ```
pub struct Pipeline<S> {
    source: Arc<S>,
    parser: Arc<dyn ProtocolParser>,
    bus: Arc<EventBus>,
    config: PipelineConfig,
}

impl<S: IqSource> Pipeline<S> {
    /// Build a new pipeline.
    ///
    /// `source` — the I/Q source (RTL-SDR, SoapySDR, test doubles…).
    /// `parser` — the protocol parser; use `()` for the default `tag(0xaa
    /// 0xbb)` recognizer.
    /// `bus` — the event bus the pipeline publishes `RadioBurstDetected`
    /// events onto.
    pub fn new(
        source: Arc<S>,
        parser: Arc<dyn ProtocolParser>,
        bus: Arc<EventBus>,
        config: PipelineConfig,
    ) -> Self {
        Self {
            source,
            parser,
            bus,
            config,
        }
    }

    /// Run the pipeline to completion.
    ///
    /// The three stages run concurrently (three tokio tasks over two
    /// `mpsc` channels). The loop ends when the source returns an empty
    /// batch; the pipeline then joins all stages and returns `Ok(())`.
    /// Any stage failure returns `MediamasterError::Other`.
    pub async fn run(self) -> Result<(), MediamasterError> {
        let source = self.source;
        let parser = self.parser;
        let bus = self.bus;
        let config = self.config;

        let (stage1_tx, mut stage1_rx) = tokio::sync::mpsc::channel(config.buffer_size);
        let (stage2_tx, stage2_rx) = tokio::sync::mpsc::channel(config.buffer_size);

        // Stage 1 — IQ acquisition (the `AcquisitionStage` facade).
        let stage_acquisition = AcquisitionStage::new(source, config.batch_size).run(stage1_tx);

        // Stage 2 — FFT + peak detection.
        let stage_dsp = async move {
            let detector = MagnitudeDetector::new(config.fft_size, config.threshold);
            while let Some(samples) = stage1_rx.recv().await {
                let magnitudes = detector.peaks(&samples);
                let mut signals = Vec::new();
                detector.detect(&magnitudes, &mut signals);
                stage2_tx
                    .send((signals, samples))
                    .await
                    .map_err(|_| MediamasterError::Other("pipeline halted".to_string()))?;
            }
            Ok(())
        };

        // Stage 3 — protocol parsing + event emission.
        let stage_parse = async move {
            let parser = parser;
            let mut rx = stage2_rx;
            let bus = bus;
            while let Some((signals, samples)) = rx.recv().await {
                let bytes = samples_to_bytes(&samples);
                for signal in signals {
                    if let Ok(header) = parser.parse_sync(&bytes) {
                        let payload_start = header.sync_word.len();
                        let payload = bytes[payload_start..]
                            [..header.payload_len.min(bytes.len() - payload_start)]
                            .to_vec();
                        let burst = RadioBurst {
                            id: signal.id,
                            frequency_hz: config.center_frequency_hz,
                            power_db: signal.magnitude_peak,
                            payload,
                            detected_by: "mediamaster-rf pipeline".to_string(),
                        };
                        let _ = bus.publish(HardwareEvent::RadioBurstDetected { burst });
                    }
                }
            }
            Ok(())
        };

        match tokio::try_join![stage_acquisition, stage_dsp, stage_parse] {
            Ok(((), (), ())) => Ok(()),
            Err(_) => Err(MediamasterError::Other("pipeline stage failed".to_string())),
        }
    }
}

/// Convert I/Q samples to a byte stream for the protocol parser.
///
/// Placeholder for the demodulator: payload bytes are carried in the real
/// part of the I/Q samples (one byte per sample, values 0–255). A real
/// front-end replaces this function with its native payload extraction; the
/// rest of the pipeline is unchanged.
pub fn samples_to_bytes(samples: &[IqSample]) -> Vec<u8> {
    samples.iter().map(|s| s.re as i32 as u8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_to_bytes_recovers_payload_bytes() {
        let samples = vec![
            IqSample::new(0xaa as f32, 0.0),
            IqSample::new(0xbb as f32, 0.0),
            IqSample::new(0x01 as f32, 0.0),
            IqSample::new(0x02 as f32, 0.0),
            IqSample::new(0x03 as f32, 0.0),
            IqSample::new(0x04 as f32, 0.0),
        ];
        let bytes = samples_to_bytes(&samples);
        assert_eq!(
            bytes,
            vec![0xaa, 0xbb, 0x01, 0x02, 0x03, 0x04],
            "placeholder byte conversion must be exact for 0-255"
        );
    }
}
