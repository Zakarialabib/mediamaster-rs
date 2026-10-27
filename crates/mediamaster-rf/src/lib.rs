//! RF signal processing pipeline — 3-stage, tokio-async.

pub mod detector;
pub mod iq_source;
pub mod parser;
pub mod pipeline;

pub use detector::MagnitudeDetector;
pub use iq_source::{IqSample, IqSource};
pub use mediamaster_core::traits::SignalDetector;
pub use parser::{ProtocolKind, ProtocolParser};
pub use pipeline::AcquisitionStage;
