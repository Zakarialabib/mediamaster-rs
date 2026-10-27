//! Three-stage async pipeline over an I/Q source:
//!
//! 1. **IQ Acquisition** — blocking task reads raw I/Q samples from SDR
//!    hardware (via `rtlsdr`/`soapysdr`), batches them, and forwards to
//!    stage 2.
//! 2. **DSP & Detection** — async task applies FFT, scans the magnitude
//!    array for peaks.
//! 3. **Protocol Parsing** — async task uses `nom` combinators to parse
//!    binary protocols and emits `RadioBurstDetected` events.
//!
//! The read side is blocking; it bounds the batch size and yields an
//! `IqBatch` into the next stage.

use mediamaster_core::MediamasterError;

use crate::iq_source::IqSource;
use crate::iq_source::IqSample;

/// Stage 1 — IQ Acquisition: blocking task reads raw I/Q samples from SDR
/// hardware, batches them, and forwards to stage 2.
pub struct AcquisitionStage<S> {
    source: S,
    batch_size: usize,
}

impl<S: IqSource> AcquisitionStage<S> {
    pub fn new(source: S, batch_size: usize) -> Self {
        Self { source, batch_size }
    }

    /// Run the acquisition loop until the source yields an empty batch.
    pub async fn run(self) -> Result<(), MediamasterError> {
        loop {
            let _batch: Vec<IqSample> = self.source.acquire(self.batch_size).await;
            // A real implementation forwards `batch` to stage 2 via an mpsc
            // channel. Here we surface the acquired batch for demo purposes.
            if _batch.is_empty() {
                break;
            }
        }
        Ok(())
    }
}
