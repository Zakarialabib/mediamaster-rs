//! End-to-end: OOK I/Q source → demod → protocol parse → `EventBus` burst.

use std::sync::{Arc, Mutex};

use num_complex::Complex;

use mediamaster_core::events::{EventBus, HardwareEvent};
use mediamaster_rf::{IqSample, IqSource, OokDemodulator, Pipeline, PipelineConfig};

/// A synthetic OOK source that emits a fixed byte stream once, then reports
/// end-of-stream (an empty batch).
struct OokTestSource {
    stream: Vec<IqSample>,
    cursor: Mutex<usize>,
}

impl OokTestSource {
    /// Encode `bytes` as OOK I/Q: bit 1 → full-scale carrier, bit 0 → silence,
    /// each bit spanning `sps` samples.
    fn new(bytes: &[u8], sps: usize) -> Self {
        let mut stream = Vec::with_capacity(bytes.len() * 8 * sps);
        for &b in bytes {
            for i in (0..8).rev() {
                let amp = if (b >> i) & 1 == 1 { 1.0 } else { 0.0 };
                for _ in 0..sps {
                    stream.push(Complex::new(amp, 0.0));
                }
            }
        }
        Self {
            stream,
            cursor: Mutex::new(0),
        }
    }
}

impl IqSource for OokTestSource {
    async fn acquire(&self, count: usize) -> Vec<IqSample> {
        let mut cursor = self.cursor.lock().unwrap();
        if *cursor >= self.stream.len() {
            return Vec::new();
        }
        let end = (*cursor + count).min(self.stream.len());
        let batch = self.stream[*cursor..end].to_vec();
        *cursor = end;
        batch
    }

    fn sample_rate_hz(&self) -> u32 {
        1_000_000
    }
}

#[tokio::test]
async fn ook_pipeline_publishes_detected_burst_with_payload() {
    let frame = [0xaa, 0xbb, 0x01, 0x02];
    let sps = 4;
    let stream_len = frame.len() * 8 * sps;

    let bus = Arc::new(EventBus::new(16));
    let mut rx = bus.subscribe();

    let config = PipelineConfig {
        batch_size: stream_len,
        fft_size: stream_len,
        threshold: 0.0,
        buffer_size: 4,
        center_frequency_hz: 433_920_000,
    };

    let pipeline = Pipeline::new(
        Arc::new(OokTestSource::new(&frame, sps)),
        Arc::new(()),
        Arc::new(OokDemodulator::new(0.5, sps)),
        bus,
        config,
    );
    pipeline.run().await.expect("pipeline runs to completion");

    let event = rx
        .try_recv()
        .expect("a RadioBurstDetected event was published");
    match event {
        HardwareEvent::RadioBurstDetected { burst } => {
            assert_eq!(burst.payload, vec![0x01, 0x02]);
            assert_eq!(burst.frequency_hz, 433_920_000);
            assert_eq!(burst.detected_by, "OOK@433920000Hz");
            assert!(burst.power_db > 0.0);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test]
async fn ook_pipeline_quiet_source_publishes_nothing() {
    // A source that emits one all-silence batch then end-of-stream: no energy →
    // no detected signal → no burst published.
    struct QuietOnce {
        sent: Mutex<bool>,
    }
    impl IqSource for QuietOnce {
        async fn acquire(&self, _count: usize) -> Vec<IqSample> {
            let mut sent = self.sent.lock().unwrap();
            if *sent {
                Vec::new()
            } else {
                *sent = true;
                vec![Complex::new(0.0, 0.0); 64]
            }
        }
        fn sample_rate_hz(&self) -> u32 {
            1_000_000
        }
    }

    let bus = Arc::new(EventBus::new(16));
    let mut rx = bus.subscribe();
    let config = PipelineConfig {
        batch_size: 64,
        fft_size: 64,
        threshold: 0.5,
        buffer_size: 4,
        center_frequency_hz: 433_920_000,
    };
    let pipeline = Pipeline::new(
        Arc::new(QuietOnce {
            sent: Mutex::new(false),
        }),
        Arc::new(()),
        Arc::new(OokDemodulator::new(0.5, 4)),
        bus,
        config,
    );
    pipeline.run().await.expect("pipeline runs to completion");
    assert!(
        rx.try_recv().is_err(),
        "a silent source must not publish a burst"
    );
}
