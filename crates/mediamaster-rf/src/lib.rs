//! RF signal processing pipeline — 3-stage, tokio-async.
//!
//! [See the module-level documentation](crate::pipeline) for the pipeline
//! design, the OOK/ASK demodulator, and a usage example.

pub mod demod;
pub mod detector;
pub mod iq_source;
pub mod parser;
pub mod pipeline;

pub use demod::OokDemodulator;
pub use detector::MagnitudeDetector;
pub use iq_source::{IqSample, IqSource};
pub use mediamaster_core::traits::SignalDetector;
pub use parser::{ProtocolKind, ProtocolParser};
pub use pipeline::{AcquisitionStage, Pipeline, PipelineConfig};
