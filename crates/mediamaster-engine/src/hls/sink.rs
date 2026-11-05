//! `SegmentSink` — trait and types for delivering produced HLS segments.
//!
//! The boundary law is explicit: mediamaster PRODUCES compliant segments;
//! it NEVER hosts HTTP servers or serves bytes directly. Segment bytes go out
//! through [`SegmentSink::put_segment`], which the embedding application wires
//! to its own storage, ring buffer, or HTTP byte server (e.g. `tv-rs-media`).

use std::collections::VecDeque;
use std::sync::Mutex;

/// The category of a produced segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentKind {
    /// Pure media (e.g. video-only or audio-only ES/TS or fMP4 media fragment).
    Media,
    /// Multiplexed media (e.g. audio + video MPEG-TS).
    Muxed,
    /// Initialization segment (e.g. fMP4 CMAF init header `ftyp` + `moov`).
    Init,
    /// Partial segment for Low-Latency HLS (EXT-X-PART).
    Part,
    /// Static fallback or poster image artifact.
    Poster,
}

/// A completed HLS segment or fragment ready for publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Monotonically increasing identifier / sequence index within the rendition.
    pub id: u64,
    /// Segment category.
    pub kind: SegmentKind,
    /// Suggested URI or filename (e.g. "stream_0_123.ts" or "init.mp4").
    pub uri_hint: String,
    /// Payload bytes (backed by FramePacket payloads or muxed container bytes).
    pub body: Vec<u8>,
    /// Duration of the segment in milliseconds (0 for Init segments).
    pub duration_ms: u64,
}

impl Segment {
    /// Creates a new segment.
    pub fn new(
        id: u64,
        kind: SegmentKind,
        uri_hint: impl Into<String>,
        body: Vec<u8>,
        duration_ms: u64,
    ) -> Self {
        Self {
            id,
            kind,
            uri_hint: uri_hint.into(),
            body,
            duration_ms,
        }
    }
}

/// Trait implemented by embedding applications or storage sinks to receive
/// completed HLS segments.
pub trait SegmentSink: Send + Sync {
    /// Emit a completed segment.
    fn put_segment(&self, seg: Segment) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// An in-memory rolling window sink that stores the most recent `N` segments.
#[derive(Debug)]
pub struct InMemoryRollingSink {
    window_size: usize,
    segments: Mutex<VecDeque<Segment>>,
}

impl InMemoryRollingSink {
    /// Create a new rolling sink keeping up to `window_size` segments in memory.
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            segments: Mutex::new(VecDeque::with_capacity(window_size)),
        }
    }

    /// Snapshot all segments currently in the rolling window.
    pub fn snapshot(&self) -> Vec<Segment> {
        self.segments.lock().unwrap().iter().cloned().collect()
    }

    /// Count of segments currently retained.
    pub fn len(&self) -> usize {
        self.segments.lock().unwrap().len()
    }

    /// True if no segments are held.
    pub fn is_empty(&self) -> bool {
        self.segments.lock().unwrap().is_empty()
    }
}

impl SegmentSink for InMemoryRollingSink {
    fn put_segment(&self, seg: Segment) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut guard = self.segments.lock().unwrap();
        if guard.len() >= self.window_size && self.window_size > 0 {
            guard.pop_front();
        }
        guard.push_back(seg);
        Ok(())
    }
}
