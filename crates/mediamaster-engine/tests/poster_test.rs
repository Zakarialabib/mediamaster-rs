use mediamaster_bus::EventBus;
use mediamaster_core::caps::{CapsMatrix, DegradationTier};
use mediamaster_core::events::HardwareEvent;
use mediamaster_core::policy::{BackdropPolicy, BackdropSelect};
use mediamaster_engine::{
    BackdropSink, EncodedPacket, PacketSource, PosterProducer, SoftwareCapsDiscovery,
};
use mediamaster_probe::types::{Container, MediaDescriptor, VideoCodec, VideoInfo};
use std::error::Error;

struct SyntheticPacketSource {
    packets: Vec<EncodedPacket>,
    idx: usize,
}

impl SyntheticPacketSource {
    fn new(packets: Vec<EncodedPacket>) -> Self {
        Self { packets, idx: 0 }
    }
}

impl PacketSource for SyntheticPacketSource {
    fn next_packet(&mut self) -> Result<Option<EncodedPacket>, Box<dyn Error + Send + Sync>> {
        if self.idx < self.packets.len() {
            let p = self.packets[self.idx].clone();
            self.idx += 1;
            Ok(Some(p))
        } else {
            Ok(None)
        }
    }
}

#[derive(Default)]
struct MemoryBackdropSink {
    emitted: Vec<(DegradationTier, String, Vec<u8>, String)>,
}

impl BackdropSink for MemoryBackdropSink {
    fn emit_backdrop(
        &mut self,
        tier: DegradationTier,
        mime: &str,
        data: &[u8],
        uri_hint: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.emitted
            .push((tier, mime.to_string(), data.to_vec(), uri_hint.to_string()));
        Ok(())
    }
}

#[test]
fn test_poster_producer_midpoint_selection() {
    let bus = EventBus::new(10);
    let mut rx = bus.subscribe();

    // 5 synthetic keyframe packets with distinct PTS and distinct content
    let packets = vec![
        EncodedPacket {
            data: vec![10, 10, 10],
            is_keyframe: true,
            pts: 100,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![20, 20, 20],
            is_keyframe: true,
            pts: 200,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![30, 30, 30], // Midpoint index = 5 / 2 = 2
            is_keyframe: true,
            pts: 300,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![40, 40, 40],
            is_keyframe: true,
            pts: 400,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![50, 50, 50],
            is_keyframe: true,
            pts: 500,
            stream_id: "stream_1".into(),
        },
    ];

    let source = SyntheticPacketSource::new(packets);
    let sink = MemoryBackdropSink::default();
    let caps = CapsMatrix::default();

    let mut producer = PosterProducer::new(source, sink, caps);
    let selector = SoftwareCapsDiscovery::new();

    let mut probe = MediaDescriptor::new(Container::Mp4);
    probe.video = Some(VideoInfo::new(VideoCodec::H264));

    let policy = BackdropPolicy {
        tiers: vec![DegradationTier::Tier4PosterFallback],
        max_edge_px: 1280,
        mime_budget: vec!["image/jpeg".into()],
        select_at: BackdropSelect::Midpoint,
    };

    let tier = producer
        .produce_poster(
            &selector,
            &probe,
            &policy,
            "fallback://poster/stream1",
            Some(&bus),
        )
        .expect("poster generation should succeed");

    assert_eq!(tier, DegradationTier::Tier4PosterFallback);

    // Verify sink output
    let emitted = &producer.sink().emitted;
    assert_eq!(emitted.len(), 1);
    let (emitted_tier, mime, data, uri) = &emitted[0];
    assert_eq!(*emitted_tier, DegradationTier::Tier4PosterFallback);
    assert_eq!(mime, "image/jpeg");
    assert_eq!(uri, "fallback://poster/stream1");

    // JPEG header prefix: [0xFF, 0xD8, 0xFF, 0xE0] + max_edge_px (1280 u32 be) + payload [30, 30, 30]
    let mut expected_prefix = vec![0xFF, 0xD8, 0xFF, 0xE0];
    expected_prefix.extend_from_slice(&1280u32.to_be_bytes());
    expected_prefix.extend_from_slice(&[30, 30, 30]);
    assert_eq!(data, &expected_prefix);

    // Verify bus event
    let event = rx.try_recv().expect("event received on bus");
    assert_eq!(
        event,
        HardwareEvent::PosterGenerated {
            uri_hint: "fallback://poster/stream1".into(),
            tier: DegradationTier::Tier4PosterFallback,
        }
    );
}

#[test]
fn test_poster_producer_sharpest_selection() {
    // 3 keyframes:
    // 1) Flat uniform: [50, 50, 50, 50] -> var = 0
    // 2) Moderate: [40, 50, 60, 50]
    // 3) High contrast / sharp: [0, 255, 0, 255] -> highest variance
    let packets = vec![
        EncodedPacket {
            data: vec![50, 50, 50, 50],
            is_keyframe: true,
            pts: 100,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![0, 255, 0, 255],
            is_keyframe: true,
            pts: 200,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![40, 50, 60, 50],
            is_keyframe: true,
            pts: 300,
            stream_id: "stream_1".into(),
        },
    ];

    let source = SyntheticPacketSource::new(packets);
    let sink = MemoryBackdropSink::default();
    let caps = CapsMatrix::default();

    let mut producer = PosterProducer::new(source, sink, caps);
    let selector = SoftwareCapsDiscovery::new();

    let mut probe = MediaDescriptor::new(Container::Webm);
    probe.video = Some(VideoInfo::new(VideoCodec::Av1));

    let policy = BackdropPolicy {
        tiers: vec![DegradationTier::Tier4PosterFallback],
        max_edge_px: 1920,
        mime_budget: vec!["image/webp".into()],
        select_at: BackdropSelect::Sharpest,
    };

    let tier = producer
        .produce_poster(&selector, &probe, &policy, "fallback://poster/sharp", None)
        .expect("poster generation should succeed");

    assert_eq!(tier, DegradationTier::Tier4PosterFallback);

    let emitted = &producer.sink().emitted;
    assert_eq!(emitted.len(), 1);
    let (_, mime, data, _) = &emitted[0];
    assert_eq!(mime, "image/webp");
    // WebP header prefix: RIFF....WEBP + 1920u32 be + payload [0, 255, 0, 255]
    assert!(data.ends_with(&[0, 255, 0, 255]));
}

#[test]
fn test_poster_producer_first_keyframe_selection() {
    let packets = vec![
        EncodedPacket {
            data: vec![1, 2, 3],
            is_keyframe: false, // Not a keyframe, must be skipped
            pts: 50,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![7, 8, 9],
            is_keyframe: true, // First keyframe
            pts: 100,
            stream_id: "stream_1".into(),
        },
        EncodedPacket {
            data: vec![10, 11, 12],
            is_keyframe: true,
            pts: 200,
            stream_id: "stream_1".into(),
        },
    ];

    let source = SyntheticPacketSource::new(packets);
    let sink = MemoryBackdropSink::default();
    let caps = CapsMatrix::default();

    let mut producer = PosterProducer::new(source, sink, caps);
    let selector = SoftwareCapsDiscovery::new();

    let mut probe = MediaDescriptor::new(Container::Mp4);
    probe.video = Some(VideoInfo::new(VideoCodec::Hevc));

    let policy = BackdropPolicy {
        tiers: vec![DegradationTier::Tier4PosterFallback],
        max_edge_px: 720,
        mime_budget: vec!["image/jpeg".into()],
        select_at: BackdropSelect::FirstKeyframe,
    };

    producer
        .produce_poster(&selector, &probe, &policy, "fallback://first", None)
        .expect("produce_poster");

    let emitted = &producer.sink().emitted;
    assert_eq!(emitted.len(), 1);
    assert!(emitted[0].2.ends_with(&[7, 8, 9]));
}
