//! Example: wiring `SegmentSink` to an in-memory rolling buffer without disk/network IO.
//!
//! Demonstrates the explicit architectural boundary:
//! `mediamaster-engine` produces compliant HLS segments;
//! it NEVER hosts HTTP servers or serves bytes directly.
//! The embedding app receives segments through the `SegmentSink` trait.

use mediamaster_engine::hls::{
    HlsContainer, HlsSegmenter, HlsSegmenterConfig, InMemoryRollingSink, Segment, SegmentSink,
};
use mediamaster_frame::{CodecFormat, FramePacket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Custom application sink wrapping an in-memory store and counting segments.
struct AppCustomSink {
    inner_sink: InMemoryRollingSink,
    segment_count: AtomicUsize,
}

impl AppCustomSink {
    fn new(window: usize) -> Self {
        Self {
            inner_sink: InMemoryRollingSink::new(window),
            segment_count: AtomicUsize::new(0),
        }
    }
}

impl SegmentSink for AppCustomSink {
    fn put_segment(&self, seg: Segment) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let count = self.segment_count.fetch_add(1, Ordering::SeqCst) + 1;
        println!(
            "[{:?}] Received Segment #{} (id: {}, uri: '{}', {} ms, {} bytes)",
            seg.kind,
            count,
            seg.id,
            seg.uri_hint,
            seg.duration_ms,
            seg.body.len()
        );
        self.inner_sink.put_segment(seg)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== mediamaster-engine HLS: SegmentSink Wiring to In-Memory Store ===");

    // 1. Configure the segmenter
    let config = HlsSegmenterConfig {
        target_duration_ms: 1000,
        container: HlsContainer::Ts,
        low_latency: false,
        part_target_duration_ms: 500,
        segment_prefix: "live_stream".to_string(),
        width: 1920,
        height: 1080,
        bitrate_bps: 2_500_000,
    };
    let mut segmenter = HlsSegmenter::new(config);

    // 2. Wire up the embedding application's sink
    let app_sink = Arc::new(AppCustomSink::new(5));

    // 3. Push encoded packets into the segmenter
    // We simulate 60 frames @ 30fps (2 seconds total), with IDRs at frame 0 and frame 30
    for i in 0..60 {
        let pts = (i as i64) * 3000; // 90kHz timescale (3000 ticks = 33.3ms)
        let is_idr = i % 30 == 0;
        let nalu_type = if is_idr { 0x65 } else { 0x41 };
        let mut data = vec![0, 0, 0, 1, nalu_type];
        data.extend_from_slice(b"sample_nalu_payload");

        let pkt = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            pts,
            pts,
            90_000,
            3000,
            is_idr,
            data,
        );

        segmenter.push_packet(pkt, app_sink.as_ref())?;
    }

    segmenter.finish(app_sink.as_ref())?;

    // 4. Generate the Media Playlist
    let playlist = segmenter.generate_media_playlist(true);
    println!("\nGenerated Playlist (.m3u8):\n{}", playlist);

    let retained = app_sink.inner_sink.snapshot();
    println!("Retained segments in memory: {}", retained.len());
    assert_eq!(retained.len(), 2);

    println!("HLS segment production completed successfully without disk or network IO.");
    Ok(())
}
