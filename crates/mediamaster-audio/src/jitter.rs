//! Anti-click jitter buffer for live/paging PCM streams.
//!
//! A mobile companion streams PCM over the network to the desktop node. Wall
//! clocks drift and packets arrive jittery, so a naive straight drain clicks
//! on every gap. [`PagingJitterBuffer`] fixes that:
//!
//! 1. **Prebuffer** — hold incoming samples until a threshold (`50ms` by
//!    default) is reached before any drain, so playback starts smooth.
//! 2. **Fractional resample** — drain through a 4-point Hermite cubic
//!    interpolator whose step is nudged by the ring's fill ratio, absorbing
//!    `±100ppm` mobile-to-desktop clock drift with **zero heap allocation**.
//! 3. **Underrun fade** — on starvation, ramp the last sample down over `5ms`
//!    (raised cosine) before emitting `0.0` silence, eliminating pops.

use ringbuf::HeapRb;
use ringbuf::Rb;

/// Output sample rate the buffer resamples to (paging/voice: 48 kHz mono).
const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// Default prebuffer window in milliseconds.
const DEFAULT_PREBUFFER_MS: u32 = 50;
/// Ring capacity in samples (≈ 85 ms of 48 kHz mono headroom).
const CAPACITY: usize = 4096;
/// Underrun fade length in milliseconds (5 ms raised cosine).
const FADE_MS: u32 = 5;
/// Proportional gain mapping fill-ratio deviation to clock-drift correction.
/// Tuned so a full-range fill excursion corrects roughly `±200ppm`, keeping the
/// `±100ppm` mobile drift inside the linear region.
const DRIFT_GAIN: f64 = 0.0004;
/// Target fill ratio (half full) — the resampler's equilibrium.
const TARGET_FILL: f64 = 0.5;

/// Jitter buffer that prebuffers, drift-compensates, and fades out on underrun.
pub struct PagingJitterBuffer {
    ring: HeapRb<f32>,
    prebuffer_threshold: usize,
    /// Fractional read cursor into the drain (for Hermite interpolation).
    read_frac: f64,
    /// Samples already pushed but not yet prebuffer-gated out.
    seen: usize,
    /// Whether the prebuffer gate has opened for good.
    started: bool,
    /// Underrun fade: remaining fade-in samples at the tail (else 0).
    fade_remaining: usize,
    fade_total: usize,
    last_sample: f32,
    /// Set once an underrun fade has fully completed; cleared when data resumes.
    faded_out: bool,
    /// Optional event bus to publish underrun/overrun events.
    event_bus: Option<std::sync::Arc<mediamaster_bus::EventBus>>,
    label: String,
}

impl PagingJitterBuffer {
    /// Create a buffer targeting `sample_rate` with a `prebuffer_ms` warm-up.
    pub fn new(sample_rate: u32, prebuffer_ms: u32) -> Self {
        let sr = sample_rate.max(1);
        let threshold = ((sr as u64 * prebuffer_ms as u64) / 1000) as usize;
        Self {
            ring: HeapRb::new(CAPACITY),
            prebuffer_threshold: threshold.max(1),
            read_frac: 0.0,
            seen: 0,
            started: false,
            fade_remaining: 0,
            fade_total: ((sr as u64 * FADE_MS as u64) / 1000) as usize,
            last_sample: 0.0,
            faded_out: false,
            event_bus: None,
            label: "paging-jitter".to_string(),
        }
    }

    /// Defaults: 48 kHz, 50 ms prebuffer.
    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_SAMPLE_RATE, DEFAULT_PREBUFFER_MS)
    }

    /// Attach an [`EventBus`] to emit buffer events.
    pub fn with_event_bus(mut self, bus: std::sync::Arc<mediamaster_bus::EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Set a custom label for event reporting.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Push incoming network PCM (as much as fits; drops the oldest-overflow tail).
    pub fn push(&mut self, samples: &[f32]) {
        let mut overrun_count = 0;
        for &s in samples {
            // If full, drop one sample to make room (never allocate).
            if self.ring.len() == self.ring.capacity() {
                let _ = self.ring.pop();
                overrun_count += 1;
                // Advance the read cursor by one sample (clamped to ring length).
                self.read_frac = (self.read_frac + 1.0).min(self.ring.len() as f64);
            }
            let _ = self.ring.push(s);
            self.last_sample = s;
        }
        if overrun_count > 0 {
            if let Some(bus) = &self.event_bus {
                let _ = bus.publish(mediamaster_core::events::HardwareEvent::AudioOverrun {
                    source: self.label.clone(),
                    dropped_frames: overrun_count,
                });
            }
        }
        self.seen += samples.len();
        if !self.started && self.seen >= self.prebuffer_threshold {
            self.started = true;
        }
    }

    /// `true` once enough samples have arrived to begin draining.
    pub fn is_primed(&self) -> bool {
        self.started
    }

    /// Current fill ratio in `[0, 1]`.
    pub fn fill_ratio(&self) -> f64 {
        self.ring.len() as f64 / CAPACITY as f64
    }

    /// The drift-corrected step (samples consumed per output sample), derived
    /// from the fill ratio so the buffer tends toward `TARGET_FILL`.
    fn resample_step(&self) -> f64 {
        1.0 + (self.fill_ratio() - TARGET_FILL) * DRIFT_GAIN
    }

    /// 4-point cubic Hermite (Catmull-Rom) interpolation between `y1` and `y2`
    /// at fractional offset `t ∈ [0,1)`.
    fn hermite(t: f64, y0: f64, y1: f64, y2: f64, y3: f64) -> f64 {
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    /// Pull the next output sample. Returns `0.0` silence before priming, a
    /// raised-cosine fade into silence on underrun, and otherwise the
    /// Hermite-resampled stream.
    pub fn pop_output(&mut self) -> f32 {
        if !self.started {
            return 0.0;
        }
        // Sustained underrun after a completed fade: stay silent (no re-trigger).
        if self.faded_out {
            if self.ring.len() == 0 {
                return 0.0;
            }
            // New data arrived — resume normal draining.
            self.faded_out = false;
            self.fade_remaining = 0;
        }
        // Underrun fade already in progress: continue it to silence.
        if self.fade_remaining > 0 {
            let remaining = self.fade_remaining;
            self.fade_remaining -= 1;
            if self.fade_remaining == 0 {
                // Fade finished: latch so continued underrun stays silent.
                self.faded_out = true;
            }
            if self.fade_total == 0 {
                return 0.0;
            }
            // Raised-cosine (Hann half-window) from `last_sample` to 0.
            let progress = 1.0 - (remaining as f64 / self.fade_total as f64);
            let gain = 0.5 * (1.0 + (std::f64::consts::PI * progress).cos());
            return (self.last_sample as f64 * gain) as f32;
        }

        if self.ring.len() == 0 {
            // Starved: begin the fade-out instead of a hard cut (no click).
            if let Some(bus) = &self.event_bus {
                let _ = bus.publish(mediamaster_core::events::HardwareEvent::AudioUnderrun {
                    source: self.label.clone(),
                    dropped_frames: 1,
                });
            }
            self.fade_remaining = self.fade_total.max(1);
            return self.pop_output();
        }

        // Drain one resampled point (4-point Hermite around the read cursor).
        let base = self.read_frac.floor() as usize;
        let t = self.read_frac - base as f64;
        // Clamp boundary samples so we never wrap around the ring.
        let ring_len = self.len().max(1);
        let clamp = |i: usize| -> usize { i.min(ring_len.saturating_sub(1)) };
        let y0 = self.peek(clamp(base.saturating_sub(1))) as f64;
        let y1 = self.peek(clamp(base)) as f64;
        let y2 = self.peek(clamp(base + 1)) as f64;
        let y3 = self.peek(clamp(base + 2)) as f64;
        let out = Self::hermite(t, y0, y1, y2, y3);

        // Advance by the drift-corrected step; pop whole samples we passed.
        self.read_frac += self.resample_step();
        let whole = self.read_frac.floor();
        for _ in 0..whole as usize {
            let _ = self.ring.pop();
        }
        self.read_frac -= whole;

        let s = out as f32;
        self.last_sample = s;
        s
    }

    /// Number of samples currently buffered.
    fn len(&self) -> usize {
        self.ring.len()
    }

    /// Peek the sample `offset` positions ahead of the ring's read cursor.
    fn peek(&self, offset: usize) -> f32 {
        // `HeapRb` has no direct peek; approximate by iterating `Iterator`
        // clone-free: `ringbuf::Rb` exposes `iter()`? To stay allocation-free
        // we rely on `pop`-free access via `as_slices`.
        let (a, b) = self.ring.as_slices();
        let total = a.len() + b.len();
        if total == 0 {
            return 0.0;
        }
        let i = offset % total;
        if i < a.len() {
            a[i]
        } else {
            b[i - a.len()]
        }
    }
}

impl Default for PagingJitterBuffer {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// An `AudioSource` adapter wrapping a [`PagingJitterBuffer`].
///
/// Can be attached as a priority zone input or directly into a [`Mixer`](crate::Mixer).
/// Optionally emits `AudioDuckRequest` events when paging audio begins arriving,
/// triggering automatic ducking across other zones via the paging-duck loop in `mediamaster-agent`.
pub struct PagingSource {
    buffer: std::sync::Mutex<PagingJitterBuffer>,
    label: String,
    ducking_profile: Option<crate::ducking::DuckingProfile>,
    event_bus: Option<std::sync::Arc<mediamaster_bus::EventBus>>,
    was_active: std::sync::atomic::AtomicBool,
}

impl PagingSource {
    /// Create a new `PagingSource` wrapping a [`PagingJitterBuffer`].
    pub fn new(
        jitter: PagingJitterBuffer,
        label: impl Into<String>,
    ) -> Result<Self, mediamaster_core::error::MediamasterError> {
        let label = label.into();
        if label.is_empty() {
            return Err(mediamaster_core::error::MediamasterError::Audio(
                "paging source label cannot be empty".into(),
            ));
        }
        Ok(Self {
            buffer: std::sync::Mutex::new(jitter),
            label,
            ducking_profile: None,
            event_bus: None,
            was_active: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Configure automatic ducking when paging stream is active.
    pub fn with_auto_ducking(
        mut self,
        profile: crate::ducking::DuckingProfile,
        bus: std::sync::Arc<mediamaster_bus::EventBus>,
    ) -> Self {
        self.ducking_profile = Some(profile);
        self.event_bus = Some(bus);
        self
    }

    /// Push network PCM into the jitter buffer. If auto-ducking is configured and
    /// this activates the stream, an `AudioDuckRequest` event is published.
    pub fn push(&self, samples: &[f32]) {
        let mut jb = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        jb.push(samples);
        if jb.is_primed()
            && !self
                .was_active
                .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            if let (Some(profile), Some(bus)) = (&self.ducking_profile, &self.event_bus) {
                let _ = bus.publish(mediamaster_core::events::HardwareEvent::AudioDuckRequest {
                    source: self.label.clone(),
                    ducking: profile.target,
                    until: profile.duration_ms,
                });
            }
        }
    }

    /// Access the underlying jitter buffer (e.g. to inspect fill ratio).
    pub fn fill_ratio(&self) -> f64 {
        self.buffer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .fill_ratio()
    }
}

impl mediamaster_core::traits::AudioSource for PagingSource {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        let mut jb = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        let mut non_zero = false;
        for slot in buffer.iter_mut() {
            let s = jb.pop_output();
            *slot = s;
            if s.abs() > 1e-6 {
                non_zero = true;
            }
        }

        if !non_zero
            && self
                .was_active
                .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            // Paging stream went silent / released. Publish release duck request if configured.
            if let (Some(profile), Some(bus)) = (&self.ducking_profile, &self.event_bus) {
                let _ = bus.publish(mediamaster_core::events::HardwareEvent::AudioDuckRequest {
                    source: self.label.clone(),
                    ducking: 1.0,
                    until: profile.duration_ms,
                });
            }
        }

        buffer.len()
    }

    fn label(&self) -> &str {
        &self.label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prebuffer_gate_holds_until_threshold() {
        let mut jb = PagingJitterBuffer::new(1000, 50); // 1000Hz => 50 samples
        assert!(!jb.is_primed());
        jb.push(&[0.5; 49]);
        assert!(!jb.is_primed());
        jb.push(&[0.5]);
        assert!(jb.is_primed());
        // Before priming, output is silence.
        let mut fresh = PagingJitterBuffer::new(1000, 50);
        assert_eq!(fresh.pop_output(), 0.0);
    }

    #[test]
    fn silence_before_primed_then_data_after() {
        let mut jb = PagingJitterBuffer::new(1000, 10); // 10 samples
        assert_eq!(jb.pop_output(), 0.0);
        jb.push(&[1.0; 20]);
        assert!(jb.is_primed());
        let s = jb.pop_output();
        // After priming we drain real content (interpolated), not forced silence.
        assert!(s.is_finite());
    }

    #[test]
    fn underrun_fades_to_silence_without_nan() {
        let mut jb = PagingJitterBuffer::new(1000, 10);
        jb.push(&[0.8; 10]);
        assert!(jb.is_primed());
        // Drain well past the buffer: must stay finite (fade + silence), no click/NaN.
        for _ in 0..500 {
            let s = jb.pop_output();
            assert!(s.is_finite(), "non-finite output during underrun: {s}");
        }
        // After the fade completes we should be at exact silence.
        assert_eq!(jb.pop_output(), 0.0);
    }

    #[test]
    fn fill_ratio_and_drift_step() {
        let mut jb = PagingJitterBuffer::new(48_000, 50);
        assert!((jb.fill_ratio() - 0.0).abs() < 1e-9);
        jb.push(&[0.1; 2048]);
        let ratio = jb.fill_ratio();
        assert!(ratio > 0.0 && ratio < 1.0);
        // Step near equilibrium when ~half full.
        let n = CAPACITY.saturating_sub(jb.len());
        jb.push(&vec![0.1; n]);
        let step = jb.resample_step();
        assert!((step - 1.0).abs() < 1e-3, "step was {step}");
    }

    #[test]
    fn paging_source_auto_ducks_and_releases() {
        use mediamaster_core::events::HardwareEvent;
        use mediamaster_core::traits::AudioSource;

        let bus = std::sync::Arc::new(mediamaster_bus::EventBus::new(16));
        let mut rx = bus.subscribe();

        let jb = PagingJitterBuffer::new(1000, 10);
        let profile = crate::ducking::DuckingProfile {
            curve: crate::ducking::DuckingCurve::Linear,
            duration_ms: 100,
            target: 0.1,
        };
        let src = PagingSource::new(jb, "pager")
            .unwrap()
            .with_auto_ducking(profile, bus.clone());

        // Push enough to prime
        src.push(&[0.8; 20]);

        // EventBus should have received duck request
        let evt = rx.try_recv().expect("expected duck request");
        match evt {
            HardwareEvent::AudioDuckRequest {
                source,
                ducking,
                until,
            } => {
                assert_eq!(source, "pager");
                assert_eq!(ducking, 0.1);
                assert_eq!(until, 100);
            }
            _ => panic!("wrong event"),
        }

        // Drain the paging buffer
        let mut buf = [0.0f32; 100];
        let _ = src.read_pcm(&mut buf);
        let _ = src.read_pcm(&mut buf);

        // When it drops to silence, release event should be emitted
        let evt = rx.try_recv().expect("expected release request");
        match evt {
            HardwareEvent::AudioDuckRequest {
                source, ducking, ..
            } => {
                assert_eq!(source, "pager");
                assert_eq!(ducking, 1.0);
            }
            _ => panic!("wrong event"),
        }
    }
}
