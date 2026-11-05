//! `HlsSegmenter` — cuts segments on IDR boundaries at target duration and emits
//! LL-HLS-capable playlists.
//!
//! Monotonic instant + PTS arithmetic: zero wall-clock drift.
//! Supports both MPEG-TS and fMP4 CMAF media segments, and optional
//! LL-HLS partial segments (EXT-X-PART / PRELOAD-HINT).

use crate::hls::fmp4::Fmp4Muxer;
use crate::hls::sink::{Segment, SegmentKind, SegmentSink};
use crate::hls::ts::TsMuxer;
use mediamaster_frame::{CodecFormat, FramePacket, StreamKind};

/// Muxer container kind for HLS segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlsContainer {
    Ts,
    Fmp4,
}

/// Configuration for `HlsSegmenter`.
#[derive(Debug, Clone)]
pub struct HlsSegmenterConfig {
    /// Target segment duration in milliseconds (default 2000 ms).
    pub target_duration_ms: u64,
    /// Container format (TS or fMP4).
    pub container: HlsContainer,
    /// Enable Low-Latency HLS features (EXT-X-PART, EXT-X-PRELOAD-HINT).
    pub low_latency: bool,
    /// Target partial segment duration in milliseconds (e.g. 500 ms) for LL-HLS.
    pub part_target_duration_ms: u64,
    /// Name prefix for segment URIs (e.g. "seg" -> "seg_0.ts").
    pub segment_prefix: String,
    /// Video width (for fMP4 init segment and master playlist).
    pub width: u32,
    /// Video height (for fMP4 init segment and master playlist).
    pub height: u32,
    /// Target bitrate in bps (for master playlist).
    pub bitrate_bps: u64,
}

impl Default for HlsSegmenterConfig {
    fn default() -> Self {
        Self {
            target_duration_ms: 2000,
            container: HlsContainer::Ts,
            low_latency: false,
            part_target_duration_ms: 500,
            segment_prefix: "segment".to_string(),
            width: 1920,
            height: 1080,
            bitrate_bps: 2_500_000,
        }
    }
}

/// A recorded segment entry for the playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentRecord {
    pub id: u64,
    pub uri: String,
    pub duration_sec: f64,
    pub parts: Vec<PartRecord>,
}

/// A recorded LL-HLS part entry.
#[derive(Debug, Clone, PartialEq)]
pub struct PartRecord {
    pub part_idx: u32,
    pub uri: String,
    pub duration_sec: f64,
    pub independent: bool,
}

/// Segmenter that consumes `FramePacket`s and emits segments via `SegmentSink`.
pub struct HlsSegmenter {
    config: HlsSegmenterConfig,
    ts_muxer: TsMuxer,
    fmp4_muxer: Fmp4Muxer,
    buffered_packets: Vec<FramePacket>,
    part_buffered_packets: Vec<FramePacket>,
    current_segment_id: u64,
    current_part_idx: u32,
    segment_records: Vec<SegmentRecord>,
    current_segment_parts: Vec<PartRecord>,
    last_segment_start_pts_ms: Option<i64>,
    last_part_start_pts_ms: Option<i64>,
    init_emitted: bool,
    media_sequence: u64,
}

impl HlsSegmenter {
    pub fn new(config: HlsSegmenterConfig) -> Self {
        Self {
            ts_muxer: TsMuxer::new(),
            fmp4_muxer: Fmp4Muxer::new(1, 90_000),
            buffered_packets: Vec::new(),
            part_buffered_packets: Vec::new(),
            current_segment_id: 0,
            current_part_idx: 0,
            segment_records: Vec::new(),
            current_segment_parts: Vec::new(),
            last_segment_start_pts_ms: None,
            last_part_start_pts_ms: None,
            init_emitted: false,
            media_sequence: 0,
            config,
        }
    }

    /// Retrieve the current media sequence number.
    pub fn media_sequence(&self) -> u64 {
        self.media_sequence
    }

    /// Retrieve the recorded segment list.
    pub fn segments(&self) -> &[SegmentRecord] {
        &self.segment_records
    }

    /// Emit an init segment if configured for fMP4 and not yet emitted.
    pub fn ensure_init_segment(
        &mut self,
        sink: &dyn SegmentSink,
        codec: CodecFormat,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.config.container == HlsContainer::Fmp4 && !self.init_emitted {
            let init_body =
                self.fmp4_muxer
                    .build_init_segment(codec, self.config.width, self.config.height);
            let init_seg = Segment::new(
                0,
                SegmentKind::Init,
                format!("{}_init.mp4", self.config.segment_prefix),
                init_body,
                0,
            );
            sink.put_segment(init_seg)?;
            self.init_emitted = true;
        }
        Ok(())
    }

    /// Push an encoded `FramePacket` into the segmenter.
    /// Emits completed segments (and partial segments if LL-HLS) to `sink`.
    pub fn push_packet(
        &mut self,
        pkt: FramePacket,
        sink: &dyn SegmentSink,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let is_video = pkt.kind == StreamKind::Video;
        let is_idr = is_video && pkt.is_keyframe;
        let pts_ms = pkt.pts_ms();

        // Ensure init segment if fMP4
        if self.config.container == HlsContainer::Fmp4 && !self.init_emitted {
            self.ensure_init_segment(sink, pkt.codec)?;
        }

        if self.last_segment_start_pts_ms.is_none() {
            self.last_segment_start_pts_ms = Some(pts_ms);
            self.last_part_start_pts_ms = Some(pts_ms);
        }

        let elapsed_segment_ms = (pts_ms - self.last_segment_start_pts_ms.unwrap()).max(0) as u64;

        // Cut segment on IDR boundary if target duration reached
        if is_idr
            && elapsed_segment_ms >= self.config.target_duration_ms
            && !self.buffered_packets.is_empty()
        {
            self.flush_segment(sink, pts_ms)?;
        }

        // LL-HLS: check partial segment duration
        if self.config.low_latency && !self.part_buffered_packets.is_empty() {
            let elapsed_part_ms = (pts_ms - self.last_part_start_pts_ms.unwrap()).max(0) as u64;
            if elapsed_part_ms >= self.config.part_target_duration_ms {
                self.flush_part(sink, pts_ms)?;
            }
        }

        self.buffered_packets.push(pkt.clone());
        if self.config.low_latency {
            self.part_buffered_packets.push(pkt);
        }

        Ok(())
    }

    /// Finish current segment on end-of-stream.
    pub fn finish(
        &mut self,
        sink: &dyn SegmentSink,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.buffered_packets.is_empty() {
            let last_pts = self
                .buffered_packets
                .last()
                .map(|p| p.pts_ms())
                .unwrap_or(0);
            self.flush_segment(sink, last_pts)?;
        }
        Ok(())
    }

    fn flush_part(
        &mut self,
        sink: &dyn SegmentSink,
        current_pts_ms: i64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start_pts = self.last_part_start_pts_ms.unwrap_or(current_pts_ms);
        let duration_ms = (current_pts_ms - start_pts).max(1) as u64;
        let duration_sec = duration_ms as f64 / 1000.0;
        let part_idx = self.current_part_idx;
        self.current_part_idx += 1;

        let uri = match self.config.container {
            HlsContainer::Ts => format!(
                "{}_{}.{}.ts",
                self.config.segment_prefix, self.current_segment_id, part_idx
            ),
            HlsContainer::Fmp4 => format!(
                "{}_{}.{}.m4s",
                self.config.segment_prefix, self.current_segment_id, part_idx
            ),
        };

        let body = match self.config.container {
            HlsContainer::Ts => self.ts_muxer.mux_segment(&self.part_buffered_packets),
            HlsContainer::Fmp4 => self
                .fmp4_muxer
                .build_media_segment(&self.part_buffered_packets),
        };

        let independent = self.part_buffered_packets.iter().any(|p| p.is_keyframe);
        let part_rec = PartRecord {
            part_idx,
            uri: uri.clone(),
            duration_sec,
            independent,
        };
        self.current_segment_parts.push(part_rec);

        let seg = Segment::new(
            self.current_segment_id,
            SegmentKind::Part,
            uri,
            body,
            duration_ms,
        );
        sink.put_segment(seg)?;

        self.part_buffered_packets.clear();
        self.last_part_start_pts_ms = Some(current_pts_ms);
        Ok(())
    }

    fn flush_segment(
        &mut self,
        sink: &dyn SegmentSink,
        current_pts_ms: i64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start_pts = self.last_segment_start_pts_ms.unwrap_or(current_pts_ms);
        let duration_ms = (current_pts_ms - start_pts).max(1) as u64;
        let duration_sec = duration_ms as f64 / 1000.0;
        let seg_id = self.current_segment_id;
        self.current_segment_id += 1;

        let uri = match self.config.container {
            HlsContainer::Ts => format!("{}_{}.ts", self.config.segment_prefix, seg_id),
            HlsContainer::Fmp4 => format!("{}_{}.m4s", self.config.segment_prefix, seg_id),
        };

        let body = match self.config.container {
            HlsContainer::Ts => self.ts_muxer.mux_segment(&self.buffered_packets),
            HlsContainer::Fmp4 => self.fmp4_muxer.build_media_segment(&self.buffered_packets),
        };

        let seg_rec = SegmentRecord {
            id: seg_id,
            uri: uri.clone(),
            duration_sec,
            parts: std::mem::take(&mut self.current_segment_parts),
        };
        self.segment_records.push(seg_rec);

        let seg = Segment::new(seg_id, SegmentKind::Muxed, uri, body, duration_ms);
        sink.put_segment(seg)?;

        self.buffered_packets.clear();
        self.part_buffered_packets.clear();
        self.current_part_idx = 0;
        self.last_segment_start_pts_ms = Some(current_pts_ms);
        self.last_part_start_pts_ms = Some(current_pts_ms);
        Ok(())
    }

    /// Generate media playlist (.m3u8 text).
    pub fn generate_media_playlist(&self, is_vod: bool) -> String {
        let mut lines = Vec::new();
        lines.push("#EXTM3U".to_string());
        lines.push("#EXT-X-VERSION:7".to_string());

        let target_duration_sec = self
            .segment_records
            .iter()
            .map(|s| s.duration_sec.ceil() as u64)
            .max()
            .unwrap_or(self.config.target_duration_ms / 1000)
            .max(1);

        lines.push(format!("#EXT-X-TARGETDURATION:{}", target_duration_sec));
        lines.push(format!("#EXT-X-MEDIA-SEQUENCE:{}", self.media_sequence));

        if self.config.low_latency {
            let part_target_sec = self.config.part_target_duration_ms as f64 / 1000.0;
            lines.push(format!(
                "#EXT-X-PART-INF:PART-TARGET={:.3}",
                part_target_sec
            ));
            lines.push(format!(
                "#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK={:.3}",
                part_target_sec * 3.0
            ));
        }

        if self.config.container == HlsContainer::Fmp4 {
            lines.push(format!(
                "#EXT-X-MAP:URI=\"{}_init.mp4\"",
                self.config.segment_prefix
            ));
        }

        for seg in &self.segment_records {
            if self.config.low_latency && !seg.parts.is_empty() {
                for part in &seg.parts {
                    let ind_str = if part.independent {
                        ",INDEPENDENT=YES"
                    } else {
                        ""
                    };
                    lines.push(format!(
                        "#EXT-X-PART:DURATION={:.3},URI=\"{}\"{}",
                        part.duration_sec, part.uri, ind_str
                    ));
                }
            }
            lines.push(format!("#EXTINF:{:.3},", seg.duration_sec));
            lines.push(seg.uri.clone());
        }

        if self.config.low_latency {
            let next_part_uri = match self.config.container {
                HlsContainer::Ts => format!(
                    "{}_{}.{}.ts",
                    self.config.segment_prefix, self.current_segment_id, self.current_part_idx
                ),
                HlsContainer::Fmp4 => format!(
                    "{}_{}.{}.m4s",
                    self.config.segment_prefix, self.current_segment_id, self.current_part_idx
                ),
            };
            lines.push(format!(
                "#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"{}\"",
                next_part_uri
            ));
        }

        if is_vod {
            lines.push("#EXT-X-ENDLIST".to_string());
        }

        lines.join("\n") + "\n"
    }

    /// Generate master playlist (.m3u8 text) for renditions.
    pub fn generate_master_playlist(renditions: &[MasterPlaylistEntry]) -> String {
        let mut lines = Vec::new();
        lines.push("#EXTM3U".to_string());
        lines.push("#EXT-X-VERSION:7".to_string());

        for r in renditions {
            let codecs = if r.codecs.is_empty() {
                "avc1.640029,mp4a.40.2"
            } else {
                &r.codecs
            };
            lines.push(format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{},CODECS=\"{}\"",
                r.bandwidth, r.width, r.height, codecs
            ));
            lines.push(r.uri.clone());
        }

        lines.join("\n") + "\n"
    }
}

/// Entry for master playlist rendition.
#[derive(Debug, Clone)]
pub struct MasterPlaylistEntry {
    pub uri: String,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub codecs: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hls::sink::InMemoryRollingSink;

    #[test]
    fn segmenter_cuts_on_idr_and_generates_playlist() {
        let config = HlsSegmenterConfig {
            target_duration_ms: 1000,
            container: HlsContainer::Ts,
            low_latency: false,
            part_target_duration_ms: 500,
            segment_prefix: "test".to_string(),
            width: 1920,
            height: 1080,
            bitrate_bps: 1_000_000,
        };
        let mut segmenter = HlsSegmenter::new(config);
        let sink = InMemoryRollingSink::new(10);

        // Packet 0: IDR at 0ms
        let pkt0 = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            0,
            0,
            1000,
            500,
            true,
            vec![0, 0, 0, 1, 0x65, 1, 2, 3],
        );
        // Packet 1: non-IDR at 500ms
        let pkt1 = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            500,
            500,
            1000,
            500,
            false,
            vec![0, 0, 0, 1, 0x41, 4, 5, 6],
        );
        // Packet 2: IDR at 1000ms -> triggers segment cut!
        let pkt2 = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            1000,
            1000,
            1000,
            500,
            true,
            vec![0, 0, 0, 1, 0x65, 7, 8, 9],
        );

        segmenter.push_packet(pkt0, &sink).unwrap();
        segmenter.push_packet(pkt1, &sink).unwrap();
        segmenter.push_packet(pkt2, &sink).unwrap();
        segmenter.finish(&sink).unwrap();

        assert_eq!(sink.len(), 2);
        let playlist = segmenter.generate_media_playlist(true);
        assert!(playlist.contains("#EXTM3U"));
        assert!(playlist.contains("#EXT-X-TARGETDURATION:1"));
        assert!(playlist.contains("test_0.ts"));
        assert!(playlist.contains("test_1.ts"));
        assert!(playlist.contains("#EXT-X-ENDLIST"));
    }

    #[test]
    fn low_latency_emits_part_and_preload_hint() {
        let config = HlsSegmenterConfig {
            target_duration_ms: 2000,
            container: HlsContainer::Ts,
            low_latency: true,
            part_target_duration_ms: 400,
            segment_prefix: "ll".to_string(),
            width: 1920,
            height: 1080,
            bitrate_bps: 1_000_000,
        };
        let mut segmenter = HlsSegmenter::new(config);
        let sink = InMemoryRollingSink::new(10);

        let p0 = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            0,
            0,
            1000,
            200,
            true,
            vec![0, 0, 0, 1, 0x65],
        );
        let p1 = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            500,
            500,
            1000,
            200,
            false,
            vec![0, 0, 0, 1, 0x41],
        );

        segmenter.push_packet(p0, &sink).unwrap();
        segmenter.push_packet(p1, &sink).unwrap();

        let pl = segmenter.generate_media_playlist(false);
        assert!(pl.contains("#EXT-X-PART-INF:PART-TARGET="));
        assert!(pl.contains("#EXT-X-PRELOAD-HINT:TYPE=PART"));
    }
}
