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

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("{0}")]
    Other(String),
}

impl MediamasterError {
    /// Wrap this error with additional context, returning a `Runtime` variant.
    pub fn context(self, ctx: &str) -> Self {
        Self::Runtime(format!("{ctx}: {self}"))
    }

    /// Create an `Other` error from any displayable message.
    pub fn other(msg: impl std::fmt::Display) -> Self {
        Self::Other(msg.to_string())
    }
}
