//! RF signal processing pipeline — 3-stage, tokio-async.
//!
//! [See the module-level documentation](crate::pipeline) for the pipeline
//! design, the demodulator placeholder, and a usage example.

pub mod detector;
pub mod iq_source;
pub mod parser;
pub mod pipeline;

pub use detector::MagnitudeDetector;
pub use iq_source::{IqSample, IqSource};
pub use mediamaster_core::traits::SignalDetector;
pub use parser::{ProtocolKind, ProtocolParser};
pub use pipeline::{samples_to_bytes, AcquisitionStage, Pipeline, PipelineConfig};
