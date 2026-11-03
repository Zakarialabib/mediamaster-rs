//! Error type for container/bitstream probing.

/// Errors produced while probing a media byte stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    /// The input was empty.
    #[error("empty input")]
    EmptyInput,

    /// The byte stream did not match any recognized container.
    #[error("unrecognized container")]
    UnrecognizedContainer,

    /// The stream was recognized but truncated before required boxes/elements.
    #[error("truncated {0} stream")]
    Truncated(&'static str),

    /// A box/element was malformed (bad size, missing child, invalid value).
    #[error("malformed {container}: {reason}")]
    Malformed {
        container: &'static str,
        reason: String,
    },
}

impl ProbeError {
    pub(crate) fn malformed(container: &'static str, reason: impl Into<String>) -> Self {
        Self::Malformed {
            container,
            reason: reason.into(),
        }
    }
}
