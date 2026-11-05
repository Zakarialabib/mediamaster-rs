//! `mediamaster-frame` — encoded packet and media frame vocabulary.
//!
//! Provides [`FramePacket`], [`StreamKind`], [`CodecFormat`], and channel types
//! for decoupling packet producers (demuxers, decoders, encoders) from downstream
//! consumers (segmenters, muxers, mixers).

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
