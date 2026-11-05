//! Error types for `mediamaster-rag`.

use thiserror::Error;

/// Errors produced by RAG store lookups and operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RagError {
    #[error("knowledge store is empty")]
    EmptyStore,

    #[error("no matching profile or playbook found for query")]
    NoMatchFound,

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("capacity exceeded: {0}")]
    CapacityExceeded(String),

    #[error("serialization/format error: {0}")]
    FormatError(String),

    #[error("io error: {0}")]
    IoError(String),
}
