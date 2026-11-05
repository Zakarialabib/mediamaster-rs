//! Frame-level packet descriptors, zero-copy `Arc` buffers, bounded rings, and fan-out
//! distribution for the `mediamaster-rs` ecosystem.
//!
//! # Architecture & Ownership Model
//!
//! In decoded media pipelines (e.g. video playback, camera preview, HLS segmentation,
//! thumbnail extraction), passing frames across subsystem boundaries without allocation
//! or data duplication is critical for low latency and high throughput.
//!
//! ## Who Frees What?
//!
//! - **[`mediamaster_core::FrameRef`] (Borrowed)**:
//!   `FrameRef` contains an immutable borrowed slice `&'a [u8]`. It owns no memory
//!   and performs no deallocation. It is intended for synchronous, zero-copy handoffs
//!   within the lifetime `'a` of a decoder's internal buffer or an active DMA mapping.
//!
//! - **[`FramePacket`] (Owned / Shared)**:
//!   `FramePacket` wraps metadata alongside an `Arc<[u8]>` (or pooled buffer representation).
//!   Cloning a `FramePacket` is an $O(1)$ zero-copy pointer and atomic reference count
//!   increment. The underlying buffer is deallocated when the final `Arc` reference drops.
//!
//! - **[`FramePool`] (Recycled Buffers)**:
//!   When high-resolution, uncompressed video frames (e.g. 4K NV12) are decoded at 60 fps,
//!   allocating and freeing multiple megabytes per frame triggers severe memory fragmentation
//!   and allocator contention. `FramePool` manages a bounded pool of pre-allocated buffers.
//!   Buffers checked out from `FramePool` are returned to the pool once the active
//!   consumers release their `PooledBuffer` or check it back in, keeping steady-state
//!   allocations bounded to the pool capacity after initial warmup.
//!
//! ## What Happens When a `FrameRef` Lifetime Ends Mid-Decode?
//!
//! If a producer yields a `FrameRef<'a>` that is tied to a transient decoder surface
//! (for example, a hardware decoder output buffer that will be overwritten or reclaimed
//! on the next decode step), consumers *must not* retain borrowed references beyond that step.
//! If an asynchronous consumer (such as a thumbnail generator or network segmenter) needs to
//! retain the frame across decode boundaries or thread hops, it **promotes** the `FrameRef`
//! into an owned [`FramePacket`] (via [`FramePacket::from_ref`] or using a buffer checked out
//! from [`FramePool`]). Promoting copies the slice exactly once into owned/pooled memory;
//! after that promotion, the original `FrameRef` may safely expire and the decoder surface
//! may be returned to the driver immediately.
//!
//! ## How `FramePool` Recycling Interacts With `FanOut` Clones
//!
//! When a producer fans out frames via [`FanOut`], each subscriber receives its own clone
//! of the [`FramePacket`]. All subscribers share the same underlying memory backing via
//! reference counting (`Arc`).
//! When `FramePool` is used:
//! 1. The producer acquires a buffer from `FramePool`.
//! 2. The producer decodes into or populates the buffer and builds a `FramePacket`.
//! 3. `FanOut` clones the packet across $N$ subscriber queues.
//! 4. Each subscriber consumes the frame at its own pace.
//! 5. As subscribers drop their respective `FramePacket` instances, the reference count
//!    decrements.
//! 6. When all subscribers (and the producer) have dropped their references, the buffer's
//!    reference count drops to 1 (or drops completely, depending on whether weak-ref tracking
//!    or explicit checkin is used), allowing the `FramePool` to safely reuse the buffer for a
//!    subsequent frame without reallocation.

pub mod fanout;
pub mod packet;
pub mod pool;
pub mod ring;

pub use fanout::{FanOut, FanOutSubscriber, SubscriberId};
pub use packet::FramePacket;
pub use pool::{FramePool, PooledBuffer};
pub use ring::{FrameRing, FrameRingConsumer, FrameRingProducer, OverflowPolicy};
use std::sync::mpsc::{Receiver, Sender};

/// The media stream category of a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamKind {
    Video,
    Audio,
}

/// The codec format of a packet payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecFormat {
    /// H.264 / AVC Annex-B byte stream (NALUs prefixed with 0x000001 or 0x00000001).
    H264AnnexB,
    /// H.265 / HEVC Annex-B byte stream.
    HevcAnnexB,
    /// AAC ADTS stream.
    AacAdts,
    /// Raw Opus packet.
    Opus,
}

/// An encoded compressed frame or sample packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePacket {
    /// Identifier of the stream this packet belongs to.
    pub stream_id: u32,
    /// Video or Audio.
    pub kind: StreamKind,
    /// Codec format of the payload bytes.
    pub codec: CodecFormat,
    /// Presentation timestamp in timescale units (usually 90kHz or ms).
    pub pts: i64,
    /// Decode timestamp in timescale units (usually 90kHz or ms).
    pub dts: i64,
    /// Timebase units per second (e.g., 90,000 for standard MPEG timebase, or 1,000 for ms).
    pub timescale: u32,
    /// Duration of the packet in timescale units.
    pub duration: u64,
    /// True if this packet starts with a random access point / keyframe (IDR for video).
    pub is_keyframe: bool,
    /// Raw payload bytes (e.g. Annex-B NAL units or ADTS AAC frame).
    pub data: Vec<u8>,
}

impl FramePacket {
    /// Create a new video packet.
    #[allow(clippy::too_many_arguments)]
    pub fn new_video(
        stream_id: u32,
        codec: CodecFormat,
        pts: i64,
        dts: i64,
        timescale: u32,
        duration: u64,
        is_keyframe: bool,
        data: Vec<u8>,
    ) -> Self {
        Self {
            stream_id,
            kind: StreamKind::Video,
            codec,
            pts,
            dts,
            timescale,
            duration,
            is_keyframe,
            data,
        }
    }

    /// Create a new audio packet.
    pub fn new_audio(
        stream_id: u32,
        codec: CodecFormat,
        pts: i64,
        timescale: u32,
        duration: u64,
        data: Vec<u8>,
    ) -> Self {
        Self {
            stream_id,
            kind: StreamKind::Audio,
            codec,
            pts,
            dts: pts,
            timescale,
            duration,
            is_keyframe: true,
            data,
        }
    }

    /// PTS converted to milliseconds.
    pub fn pts_ms(&self) -> i64 {
        if self.timescale == 0 {
            0
        } else {
            (self.pts as i128 * 1000 / self.timescale as i128) as i64
        }
    }

    /// DTS converted to milliseconds.
    pub fn dts_ms(&self) -> i64 {
        if self.timescale == 0 {
            0
        } else {
            (self.dts as i128 * 1000 / self.timescale as i128) as i64
        }
    }

    /// Duration converted to milliseconds.
    pub fn duration_ms(&self) -> u64 {
        if self.timescale == 0 {
            0
        } else {
            (self.duration as u128 * 1000 / self.timescale as u128) as u64
        }
    }
}

/// Sender half for a channel emitting [`FramePacket`]s.
pub type PacketSender = Sender<FramePacket>;
/// Receiver half for a channel consuming [`FramePacket`]s.
pub type PacketReceiver = Receiver<FramePacket>;

/// Create an unbuffered channel for passing [`FramePacket`]s.
pub fn packet_channel() -> (PacketSender, PacketReceiver) {
    std::sync::mpsc::channel()
}

/// Create a sync bounded channel for passing [`FramePacket`]s.
pub fn sync_packet_channel(
    bound: usize,
) -> (std::sync::mpsc::SyncSender<FramePacket>, PacketReceiver) {
    std::sync::mpsc::sync_channel(bound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_timestamp_conversion() {
        let pkt = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            180_000,
            180_000,
            90_000,
            3000,
            true,
            vec![0, 0, 0, 1, 0x65],
        );
        assert_eq!(pkt.pts_ms(), 2000);
        assert_eq!(pkt.dts_ms(), 2000);
        assert_eq!(pkt.duration_ms(), 33);
        assert!(pkt.is_keyframe);
    }

    #[test]
    fn channel_roundtrip() {
        let (tx, rx) = packet_channel();
        let pkt = FramePacket::new_audio(
            2,
            CodecFormat::AacAdts,
            90_000,
            90_000,
            1920,
            vec![0xff, 0xf1],
        );
        tx.send(pkt.clone()).unwrap();
        let received = rx.recv().unwrap();
        assert_eq!(received, pkt);
    }
}
