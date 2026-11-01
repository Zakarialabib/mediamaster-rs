//! Core error type for the mediamaster-rs ecosystem.
//!
//! `alloc`-only + `no_std`: hand-written `Display`/`Error` (no `thiserror`,
//! no `std::io`). `Error` is `core::error::Error` (stable since 1.81).

use alloc::string::String;
use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediamasterError {
    Audio(String),
    Sensor(String),
    Rf(String),
    Parse(String),
    Runtime(String),
    Unsupported(String),
    Other(String),
}

impl fmt::Display for MediamasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Audio(m) => write!(f, "audio error: {m}"),
            Self::Sensor(m) => write!(f, "sensor error: {m}"),
            Self::Rf(m) => write!(f, "rf error: {m}"),
            Self::Parse(m) => write!(f, "protocol parse error: {m}"),
            Self::Runtime(m) => write!(f, "runtime error: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Other(m) => write!(f, "{m}"),
        }
    }
}

impl core::error::Error for MediamasterError {}

impl MediamasterError {
    /// Wrap this error with additional context, returning a `Runtime` variant.
    pub fn context(self, ctx: &str) -> Self {
        Self::Runtime(alloc::format!("{ctx}: {self}"))
    }

    /// Create an `Other` error from any displayable message.
    pub fn other(msg: impl fmt::Display) -> Self {
        Self::Other(alloc::format!("{msg}"))
    }
}
