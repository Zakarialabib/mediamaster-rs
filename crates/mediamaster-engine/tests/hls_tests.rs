//! Comprehensive HLS tests:
//! 1. Golden-file test: Annex-B stream -> expected .m3u8 text + segment count / durations.
//! 2. Determinism property test: fixed input packets yield byte-identical segment bodies and playlist text across runs.
//! 3. Playlist validation against hls.js expectations: EXTM3U, EXT-X-TARGETDURATION, EXT-X-VERSION, media sequence, EXTINF, EXT-X-ENDLIST.

use mediamaster_engine::hls::{
    HlsContainer, HlsSegmenter, HlsSegmenterConfig, InMemoryRollingSink,
};
use mediamaster_engine::{SoftwareCapsDiscovery, TranscodePlan};
use mediamaster_frame::{CodecFormat, FramePacket};

/// Generate a synthetic deterministic stream of Annex-B H.264 packets (IDR every 30 frames, 30fps).
fn generate_deterministic_packets(count: usize) -> Vec<FramePacket> {
    let mut packets = Vec::with_capacity(count);
    let fps_timescale = 90_000;
    let frame_duration = 3000; // 90000 / 30 = 3000 units per frame (33.33ms)

    for i in 0..count {
        let pts = (i as i64) * frame_duration;
        let is_idr = i % 30 == 0;
        let nalu_header = if is_idr { 0x65 } else { 0x41 };
        let mut data = vec![0, 0, 0, 1, nalu_header];
        // Deterministic pseudo-payload
        for b in 0..32 {
            data.push(((i * 7 + b * 13) % 256) as u8);
        }

        packets.push(FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            pts,
            pts,
            fps_timescale,
            frame_duration as u64,
            is_idr,
            data,
        ));
    }
    packets
}

#[test]
fn test_determinism_property() {
    let packets = generate_deterministic_packets(90); // 3 seconds = 3 IDR frames (0, 30, 60)

    let run_once = || {
        let config = HlsSegmenterConfig {
            target_duration_ms: 1000,
            container: HlsContainer::Ts,
            low_latency: false,
            part_target_duration_ms: 500,
            segment_prefix: "det".to_string(),
            width: 1920,
            height: 1080,
            bitrate_bps: 2_000_000,
        };
        let mut segmenter = HlsSegmenter::new(config);
        let sink = InMemoryRollingSink::new(20);

        for pkt in &packets {
            segmenter.push_packet(pkt.clone(), &sink).unwrap();
        }
        segmenter.finish(&sink).unwrap();

        let playlist = segmenter.generate_media_playlist(true);
        let segments = sink.snapshot();
        (playlist, segments)
    };

    let (pl1, segs1) = run_once();
    let (pl2, segs2) = run_once();

    // Check byte-identical playlists
    assert_eq!(pl1, pl2);
    assert_eq!(segs1.len(), segs2.len());

    // Check byte-identical segment bodies and properties
    for (s1, s2) in segs1.iter().zip(segs2.iter()) {
        assert_eq!(s1.id, s2.id);
        assert_eq!(s1.uri_hint, s2.uri_hint);
        assert_eq!(s1.duration_ms, s2.duration_ms);
        assert_eq!(s1.body, s2.body);
    }
}

#[test]
fn test_golden_file_annex_b_to_m3u8() {
    let packets = generate_deterministic_packets(65); // 0..29 (1s), 30..59 (1s), 60..64 (rem)
    let config = HlsSegmenterConfig {
        target_duration_ms: 1000,
        container: HlsContainer::Ts,
        low_latency: false,
        part_target_duration_ms: 500,
        segment_prefix: "golden".to_string(),
        width: 1920,
        height: 1080,
        bitrate_bps: 2_000_000,
    };
    let mut segmenter = HlsSegmenter::new(config);
    let sink = InMemoryRollingSink::new(10);

    for pkt in packets {
        segmenter.push_packet(pkt, &sink).unwrap();
    }
    segmenter.finish(&sink).unwrap();

    let playlist = segmenter.generate_media_playlist(true);

    let expected_golden_playlist = "\
#EXTM3U
#EXT-X-VERSION:7
#EXT-X-TARGETDURATION:1
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:1.000,
golden_0.ts
#EXTINF:1.000,
golden_1.ts
#EXTINF:0.133,
golden_2.ts
#EXT-X-ENDLIST
";

    assert_eq!(playlist, expected_golden_playlist);
    assert_eq!(sink.len(), 3);
}

#[test]
fn test_hlsjs_compliance_validation() {
    let packets = generate_deterministic_packets(35);
    let config = HlsSegmenterConfig {
        target_duration_ms: 1000,
        container: HlsContainer::Fmp4,
        low_latency: true,
        part_target_duration_ms: 300,
        segment_prefix: "hlsjs".to_string(),
        width: 1280,
        height: 720,
        bitrate_bps: 1_500_000,
    };
    let mut segmenter = HlsSegmenter::new(config);
    let sink = InMemoryRollingSink::new(10);

    for pkt in packets {
        segmenter.push_packet(pkt, &sink).unwrap();
    }
    segmenter.finish(&sink).unwrap();

    let playlist = segmenter.generate_media_playlist(true);

    // hls.js expectations:
    assert!(playlist.starts_with("#EXTM3U\n"));
    assert!(playlist.contains("#EXT-X-VERSION:7"));
    assert!(playlist.contains("#EXT-X-TARGETDURATION:"));
    assert!(playlist.contains("#EXT-X-MEDIA-SEQUENCE:0"));
    assert!(playlist.contains("#EXT-X-MAP:URI=\"hlsjs_init.mp4\""));
    assert!(playlist.contains("#EXT-X-PART-INF:PART-TARGET="));
    assert!(playlist.contains("#EXT-X-SERVER-CONTROL:"));
    assert!(playlist.contains("#EXT-X-PART:DURATION="));
    assert!(playlist.contains("#EXTINF:"));
    assert!(playlist.contains("#EXT-X-ENDLIST"));
}

#[test]
fn test_transcode_plan_to_hls_job_integration() {
    let caps = mediamaster_core::CapsMatrix {
        codecs: vec![],
        max_hw_decoder_instances: 2,
        max_hw_encoder_instances: 1,
        verified_on_hardware: true,
    };
    let plan = TranscodePlan::for_device(1920, 1080, &caps);
    let selector = SoftwareCapsDiscovery::new();
    let mut slots = mediamaster_codec::CodecSlotManager::new(&caps);

    let job = plan.to_hls_job(&selector, &mut slots, &caps, HlsContainer::Ts);
    assert_eq!(job.rendition.width, 1920);
    assert_eq!(job.rendition.height, 1080);
    assert_eq!(job.rendition.container, HlsContainer::Ts);
    assert_eq!(job.segmenter_config.segment_prefix, "stream_1080p");
}
