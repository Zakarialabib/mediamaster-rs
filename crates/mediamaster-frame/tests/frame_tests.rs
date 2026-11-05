use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use mediamaster_core::caps::{Codec, PixelFormat};
use mediamaster_core::events::{EventPublisher, HardwareEvent};
use mediamaster_core::frame::{FrameFlags, FrameRef};
use mediamaster_frame::{FanOut, FramePacket, FramePool, FrameRing, OverflowPolicy};

struct RecordingBus {
    drops: Mutex<Vec<(usize, u64)>>,
    total_drop_events: AtomicU64,
}

impl RecordingBus {
    fn new() -> Self {
        Self {
            drops: Mutex::new(Vec::new()),
            total_drop_events: AtomicU64::new(0),
        }
    }

    fn total_dropped_frames(&self) -> u64 {
        self.drops.lock().unwrap().iter().map(|(_, c)| *c).sum()
    }

    fn drops_for_subscriber(&self, id: usize) -> u64 {
        self.drops
            .lock()
            .unwrap()
            .iter()
            .filter(|(sub_id, _)| *sub_id == id)
            .map(|(_, c)| *c)
            .sum()
    }
}

impl EventPublisher for RecordingBus {
    fn publish(&self, event: HardwareEvent) {
        if let HardwareEvent::FrameDrop {
            subscriber_id,
            count,
        } = event
        {
            self.drops.lock().unwrap().push((subscriber_id, count));
            self.total_drop_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn create_sample_packet(pts: i64, size: usize) -> FramePacket {
    let payload: Arc<[u8]> = Arc::from(vec![0xAA; size].into_boxed_slice());
    FramePacket::new(
        payload,
        pts,
        Some(pts),
        Codec::H264,
        PixelFormat::Nv12,
        1920,
        1080,
        1920,
        FrameFlags::KEY,
    )
}

#[test]
fn test_frame_ref_to_packet_roundtrip() {
    let raw = vec![0x42u8; 512];
    let frame_ref = FrameRef {
        bytes: &raw,
        pts_us: 12345,
        dts_us: Some(12000),
        codec: Codec::Hevc,
        pixel_format: PixelFormat::P010,
        width: 3840,
        height: 2160,
        stride: 3840,
        flags: FrameFlags::KEY | FrameFlags::INTRA,
    };

    let packet = FramePacket::from_ref(frame_ref.clone());
    assert_eq!(packet.len(), 512);
    assert_eq!(packet.pts_us, 12345);
    assert_eq!(packet.dts_us, Some(12000));
    assert!(packet.is_key());

    let roundtrip_ref = packet.as_ref();
    assert_eq!(roundtrip_ref.bytes, &raw[..]);
    assert_eq!(roundtrip_ref.pts_us, 12345);
    assert_eq!(roundtrip_ref.codec, Codec::Hevc);
    assert_eq!(roundtrip_ref.pixel_format, PixelFormat::P010);
    assert_eq!(roundtrip_ref.flags, FrameFlags::KEY | FrameFlags::INTRA);
}

#[test]
fn test_frame_ring_spsc_backpressure() {
    // DropNewest
    let (mut tx_dn, mut rx_dn) = FrameRing::new(4, OverflowPolicy::DropNewest);
    for i in 0..4 {
        assert!(tx_dn.push(create_sample_packet(i, 64)).is_ok());
    }
    // 5th push must fail and increment dropped_count
    assert!(tx_dn.push(create_sample_packet(4, 64)).is_err());
    assert_eq!(tx_dn.dropped_count(), 1);
    assert_eq!(rx_dn.dropped_count(), 1);

    // Drain
    for i in 0..4 {
        assert_eq!(rx_dn.pop().unwrap().pts_us, i);
    }
    assert!(rx_dn.pop().is_none());

    // OverwriteOldest
    let (mut tx_oo, mut rx_oo) = FrameRing::new(4, OverflowPolicy::OverwriteOldest);
    for i in 0..4 {
        assert!(tx_oo.push(create_sample_packet(i, 64)).is_ok());
    }
    // 5th push overwrites oldest (pts 0)
    assert!(tx_oo.push(create_sample_packet(4, 64)).is_ok());
    assert_eq!(tx_oo.dropped_count(), 1);

    // Drain: pts should be 1, 2, 3, 4
    for i in 1..=4 {
        assert_eq!(rx_oo.pop().unwrap().pts_us, i);
    }
    assert!(rx_oo.pop().is_none());
}

#[test]
fn test_frame_pool_peak_allocations_bounded_after_warmup() {
    let pool_size = 8;
    let frame_bytes = 4096;
    let pool = FramePool::new(pool_size, frame_bytes);

    // Warm up pool
    pool.warmup();
    assert_eq!(pool.total_allocations(), pool_size);
    assert_eq!(pool.free_count(), pool_size);

    let raw = vec![0x55; 1024];
    let frame_ref = FrameRef {
        bytes: &raw,
        pts_us: 0,
        dts_us: None,
        codec: Codec::H264,
        pixel_format: PixelFormat::Nv12,
        width: 640,
        height: 480,
        stride: 640,
        flags: FrameFlags::empty(),
    };

    // Simulate steady-state frame processing for 1000 frames using promote_ref and into_frame_packet
    for pts in 0..1000 {
        let mut r = frame_ref.clone();
        r.pts_us = pts;
        let packet = pool.promote_ref(&r);

        // Process packet and verify payload content
        assert_eq!(packet.pts_us, pts);
        assert_eq!(packet.payload[0], 0x55);
        assert_eq!(packet.len(), 1024);
        // packet drops here -> returns buffer to pool
    }

    // Peak allocations MUST NOT exceed pool size after warmup
    assert!(
        pool.total_allocations() <= pool_size,
        "Expected allocations <= {}, got {}",
        pool_size,
        pool.total_allocations()
    );
    assert_eq!(pool.free_count(), pool_size);
}

#[test]
fn test_fanout_mpmc_stress_with_stalled_subscriber() {
    // 1 Producer, 4 Subscribers (3 fast, 1 deliberately stalled)
    const NUM_FRAMES: usize = 500;
    const QUEUE_CAPACITY: usize = 16;

    let bus = Arc::new(RecordingBus::new());
    let mut fanout = FanOut::new(QUEUE_CAPACITY, OverflowPolicy::DropNewest).with_bus(bus.clone());

    let mut sub_fast1 = fanout.subscribe();
    let mut sub_fast2 = fanout.subscribe();
    let mut sub_fast3 = fanout.subscribe();
    let mut sub_stalled = fanout.subscribe();

    let stalled_id = sub_stalled.id();
    let done = Arc::new(AtomicBool::new(false));

    // Fast subscriber 1 thread
    let done_f1 = Arc::clone(&done);
    let h_f1 = thread::spawn(move || {
        let mut received = 0;
        while !done_f1.load(Ordering::Acquire) || !sub_fast1.is_empty() {
            if sub_fast1.pop().is_some() {
                received += 1;
            } else {
                thread::yield_now();
            }
        }
        (received, sub_fast1.dropped_count())
    });

    // Fast subscriber 2 thread
    let done_f2 = Arc::clone(&done);
    let h_f2 = thread::spawn(move || {
        let mut received = 0;
        while !done_f2.load(Ordering::Acquire) || !sub_fast2.is_empty() {
            if sub_fast2.pop().is_some() {
                received += 1;
            } else {
                thread::yield_now();
            }
        }
        (received, sub_fast2.dropped_count())
    });

    // Fast subscriber 3 thread
    let done_f3 = Arc::clone(&done);
    let h_f3 = thread::spawn(move || {
        let mut received = 0;
        while !done_f3.load(Ordering::Acquire) || !sub_fast3.is_empty() {
            if sub_fast3.pop().is_some() {
                received += 1;
            } else {
                thread::yield_now();
            }
        }
        (received, sub_fast3.dropped_count())
    });

    // Deliberately stalled subscriber thread (does NOT pop frames until the end)
    let done_stalled = Arc::clone(&done);
    let h_stalled = thread::spawn(move || {
        while !done_stalled.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(5));
        }
        // Drain any remaining buffered frames at shutdown
        let mut received = 0;
        while sub_stalled.pop().is_some() {
            received += 1;
        }
        (received, sub_stalled.dropped_count())
    });

    // Producer loop: broadcast NUM_FRAMES
    for i in 0..NUM_FRAMES {
        let packet = create_sample_packet(i as i64, 128);
        fanout.broadcast(packet);
        // Small delay to let fast subscribers keep pace while stalled subscriber buffer fills
        thread::sleep(Duration::from_micros(100));
    }

    done.store(true, Ordering::Release);

    let (recv1, drop1) = h_f1.join().unwrap();
    let (recv2, drop2) = h_f2.join().unwrap();
    let (recv3, drop3) = h_f3.join().unwrap();
    let (recv_s, drop_s) = h_stalled.join().unwrap();

    // Verify fast subscribers received most/all frames with near zero or minimal drops
    assert!(
        recv1 + (drop1 as usize) == NUM_FRAMES,
        "sub1 accounting: recv {} + drop {} != {}",
        recv1,
        drop1,
        NUM_FRAMES
    );
    assert!(
        recv2 + (drop2 as usize) == NUM_FRAMES,
        "sub2 accounting: recv {} + drop {} != {}",
        recv2,
        drop2,
        NUM_FRAMES
    );
    assert!(
        recv3 + (drop3 as usize) == NUM_FRAMES,
        "sub3 accounting: recv {} + drop {} != {}",
        recv3,
        drop3,
        NUM_FRAMES
    );

    // Stalled subscriber queue capacity is 16, so it must have dropped NUM_FRAMES - QUEUE_CAPACITY frames
    assert!(
        recv_s <= QUEUE_CAPACITY,
        "stalled subscriber received {} > queue capacity {}",
        recv_s,
        QUEUE_CAPACITY
    );
    assert!(
        drop_s > 0,
        "stalled subscriber must have dropped frames, got 0"
    );
    assert_eq!(
        recv_s + (drop_s as usize),
        NUM_FRAMES,
        "stalled subscriber total frames mismatch: recv {} + drop {} != {}",
        recv_s,
        drop_s,
        NUM_FRAMES
    );

    // Verify drop accounting on the event bus
    let bus_drops_for_stalled = bus.drops_for_subscriber(stalled_id);
    assert_eq!(
        bus_drops_for_stalled, drop_s,
        "Bus drop events count {} does not match subscriber dropped_count {}",
        bus_drops_for_stalled, drop_s
    );
    assert_eq!(bus.total_dropped_frames(), drop1 + drop2 + drop3 + drop_s);
}

#[test]
fn test_fanout_mpmc_stress_overwrite_oldest() {
    // 1 Producer, 2 Subscribers under OverwriteOldest (1 fast, 1 stalled)
    const NUM_FRAMES: usize = 200;
    const QUEUE_CAPACITY: usize = 8;

    let bus = Arc::new(RecordingBus::new());
    let mut fanout =
        FanOut::new(QUEUE_CAPACITY, OverflowPolicy::OverwriteOldest).with_bus(bus.clone());

    let mut sub_fast = fanout.subscribe();
    let mut sub_stalled = fanout.subscribe();

    let stalled_id = sub_stalled.id();
    let done = Arc::new(AtomicBool::new(false));

    let done_f = Arc::clone(&done);
    let h_fast = thread::spawn(move || {
        let mut received = 0;
        while !done_f.load(Ordering::Acquire) || !sub_fast.is_empty() {
            if sub_fast.pop().is_some() {
                received += 1;
            } else {
                thread::yield_now();
            }
        }
        received
    });

    let done_s = Arc::clone(&done);
    let h_stalled = thread::spawn(move || {
        while !done_s.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(5));
        }
        let mut frames = Vec::new();
        while let Some(pkt) = sub_stalled.pop() {
            frames.push(pkt);
        }
        (frames, sub_stalled.dropped_count())
    });

    for i in 0..NUM_FRAMES {
        fanout.broadcast(create_sample_packet(i as i64, 64));
        thread::sleep(Duration::from_micros(100));
    }

    done.store(true, Ordering::Release);

    let recv_f = h_fast.join().unwrap();
    let (stalled_frames, drop_s) = h_stalled.join().unwrap();

    assert!(recv_f > 0);
    // Stalled subscriber should contain only the latest QUEUE_CAPACITY frames
    assert_eq!(stalled_frames.len(), QUEUE_CAPACITY);
    let last_frame = stalled_frames.last().unwrap();
    assert_eq!(last_frame.pts_us, (NUM_FRAMES - 1) as i64);

    let bus_drops = bus.drops_for_subscriber(stalled_id);
    assert_eq!(bus_drops, drop_s);
    assert_eq!(drop_s as usize, NUM_FRAMES - QUEUE_CAPACITY);
}
