//! Binary protocol combinators built on `nom`.

use nom::bytes::complete::tag;
use nom::error::Error;

/// Protocol kinds this library understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolKind {
    Unknown,
    WirelessMonitor,
    Chirp433Mhz,
    Proprietary,
}

/// Parse result of a protocol header.
#[derive(Debug, Clone)]
pub struct Header {
    pub kind: ProtocolKind,
    pub sync_word: Vec<u8>,
    pub payload_len: usize,
}

/// A `nom` parser that extracts a protocol header. Users implement
/// `ProtocolParser` and register their own combinators when a new protocol is
/// supported.
pub trait ProtocolParser: Send + Sync {
    fn parse_sync<'a>(&self, input: &'a [u8]) -> Result<Header, nom::Err<Error<&'a [u8]>>>;
}

impl ProtocolParser for () {
    fn parse_sync<'a>(&self, input: &'a [u8]) -> Result<Header, nom::Err<Error<&'a [u8]>>> {
        let (_rest, _) = tag(b"\xaa\xbb")(input)?;
        Ok(Header {
            kind: ProtocolKind::Unknown,
            sync_word: vec![0xaa, 0xbb],
            payload_len: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sync_recognizes_default_sync_word() {
        let input = b"\xaa\xbb\x01\x02";
        let result = ().parse_sync(input);
        assert!(result.is_ok());
        let header = result.unwrap();
        assert_eq!(header.kind, ProtocolKind::Unknown);
        assert_eq!(header.sync_word, vec![0xaa, 0xbb]);
        assert_eq!(header.payload_len, 0);
    }

    #[test]
    fn parse_sync_rejects_missing_sync_word() {
        let input = b"\xcc\xdd\x00";
        let result = ().parse_sync(input);
        assert!(result.is_err());
    }
}
