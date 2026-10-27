//! RF signal processing pipeline — 3-stage, tokio-async.

pub mod detector;
pub mod iq_source;
pub mod parser;
pub mod pipeline;

pub use mediamaster_core::traits::SignalDetector;
pub use iq_source::IqSource;
pub use parser::{ProtocolParser, ProtocolKind};
pub use pipeline::AcquisitionStage;
