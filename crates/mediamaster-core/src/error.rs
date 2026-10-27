//! Core error type for the mediamaster-rs ecosystem.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediamasterError {
    #[error("audio error: {0}")]
    Audio(String),

    #[error("sensor error: {0}")]
    Sensor(String),

    #[error("rf error: {0}")]
    Rf(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol parse error: {0}")]
    Parse(String),

    #[error("runtime error: {0}")]
    Runtime(String),
}
