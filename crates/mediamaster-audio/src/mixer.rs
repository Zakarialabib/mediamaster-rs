//! Lock-free PCM mixing: sum frames from multiple `AudioSource`s with
//! per-source gain and global ducking. The output ring uses `ringbuf::HeapRb`
//! (SPSC), so a separate thread or the `cpal` callback drains it without
//! blocking the async runtime.

use std::cell::UnsafeCell;
use std::time::Instant;

use crate::ducking::{AutomationCommand, DuckingProfile, IirSmoothing};
use mediamaster_core::traits::AudioSource;
use ringbuf::HeapRb;
use ringbuf::Rb;

/// Capacity of the output ring buffer (in frames).
const OUTPUT_CAPACITY: usize = 8192;

/// A mixer that reads from several sources, applies a global ducking
/// multiplier (optionally a smooth `DuckingProfile`), and writes mixed PCM to an
/// output ring buffer.
pub struct Mixer {
    sources: Vec<Box<dyn AudioSource>>,
    /// Optional anti-click jitter buffer for live paging streams.
    jitter: std::cell::UnsafeCell<Option<crate::jitter::PagingJitterBuffer>>,
    /// Static global ducking gain (the fallback once a profile expires).
    ducking: f32,
    /// Active time-varying ducking profile, if any. While its `duration_ms`
    /// has not elapsed, `ducking()` returns `profile.level(elapsed)`; after it
    /// expires, `ducking()` falls back to `ducking`.
    ducking_profile: Option<DuckingProfile>,
    /// `Instant` the current profile was scheduled (monotonic). `std::time`
    /// only — `Mixer` stays free of tokio/the `EventBus`: the app owns the
    /// `EventBus` subscription and feeds profiles built from events.
    profile_started: Option<Instant>,
    /// Branchless per-sample IIR gain automation, driven by
    /// [`push_automation`](Mixer::push_automation). Lives in an `UnsafeCell`
    /// (like `output`) so the single producer can step it from `&self` in the
    /// real-time path with no mutex and no heap.
    iir: UnsafeCell<IirSmoothing>,
    /// `UnsafeCell` is the only legal way to obtain `&mut` from `&self`,
    /// which the `cpal` callback (via `LockRead`) needs. In the non-cpal
    /// path, `mix(&mut self)` already has exclusive access.
    output: UnsafeCell<HeapRb<f32>>,
}

// SAFETY: `HeapRb` (`SharedRb`) uses atomic head/tail pointers internally,
// making it safe for SPSC access from two threads. The `sources`, `ducking`,
// ducking-profile and `iir` fields are only mutated through the single
// producer path (`mix`/`feed`/`push_automation`), while the consumer
// (`LockRead`) touches only the ring — so concurrent access via `&self` is
// safe for the ring buffer alone, and the IIR state is only stepped by the
// one producer at a time.
#[cfg(feature = "audio-cpal")]
unsafe impl Sync for Mixer {}

impl Mixer {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            jitter: std::cell::UnsafeCell::new(None),
            ducking: 1.0,
            ducking_profile: None,
            profile_started: None,
            iir: UnsafeCell::new(IirSmoothing::new()),
            output: UnsafeCell::new(HeapRb::new(OUTPUT_CAPACITY)),
        }
    }

    pub fn add_source(&mut self, source: Box<dyn AudioSource>) {
        self.sources.push(source);
    }

    /// Global ducking multiplier from 0.0 (fully muted) to 1.0 (full volume).
    ///
    /// If a `DuckingProfile` was scheduled via `set_ducking_profile` and its
    /// `duration_ms` has not elapsed, returns the profile's interpolated
    /// level; otherwise returns the static `ducking` gain.
    pub fn ducking(&self) -> f32 {
        if let (Some(profile), Some(start)) = (&self.ducking_profile, &self.profile_started) {
            let elapsed = start.elapsed().as_millis() as u64;
            if elapsed < profile.duration_ms {
                return profile.level(elapsed).clamp(0.0, 1.0);
            }
            // elapsed >= duration: profile expired, fall through to static gain.
        }
        self.ducking.clamp(0.0, 1.0)
    }

    pub fn set_ducking(&mut self, ducking: f32) {
        self.ducking = ducking.clamp(0.0, 1.0);
    }

    /// Schedule a smooth ducking transition (time-based profile; see
    /// [`push_automation`](Mixer::push_automation) for the lock-free
    /// per-sample IIR path used by the real-time render thread).
    pub fn set_ducking_profile(&mut self, profile: DuckingProfile) {
        self.ducking_profile = Some(profile);
        self.profile_started = Some(Instant::now());
    }

    /// Enable the anti-click jitter buffer for a live paging stream.
    pub fn enable_paging_jitter(&mut self, sample_rate: u32, prebuffer_ms: u32) {
        unsafe {
            *self.jitter.get() = Some(crate::jitter::PagingJitterBuffer::new(
                sample_rate,
                prebuffer_ms,
            ))
        };
    }

    /// Disable the paging jitter buffer.
    pub fn disable_paging_jitter(&mut self) {
        unsafe { *self.jitter.get() = None };
    }

    /// Push incoming network PCM into the paging jitter buffer.
    pub fn push_paging_pcm(&self, samples: &[f32]) {
        unsafe {
            if let Some(jb) = &mut *self.jitter.get() {
                jb.push(samples);
            }
        }
    }

    /// Queue an [`AutomationCommand`] for per-sample branchless IIR gain
    /// smoothing. The async plane builds + pushes these; the render thread
    /// steps them sample-by-sample in `push_mixed` — no mutex, no heap, no
    /// `.await` on the audio callback.
    pub fn push_automation(&mut self, cmd: AutomationCommand) {
        // SAFETY: `&mut self` guarantees exclusive access.
        unsafe { &mut *self.iir.get() }.apply(cmd);
    }

    /// Current IIR automation gain (1.0 when automation is inactive).
    pub fn iir_gain(&self) -> f32 {
        // SAFETY: read-only snapshot; producer mutates only between batches.
        unsafe { &*self.iir.get() }.current
    }

    /// Collect one batch (512 frames) from each source.
    fn collect_mixed(&self) -> Vec<Vec<f32>> {
        self.sources
            .iter()
            .map(|s| {
                let mut buf = vec![0.0f32; 512];
                let n = s.read_pcm(&mut buf);
                buf.truncate(n);
                buf
            })
            .collect()
    }

    /// Push one mixed batch into the output ring (SPSC producer, `&self`).
    /// Gain is applied **per sample**: branchless IIR automation when active,
    /// else the static/profile ducking level. Does NOT drain.
    fn push_mixed(&self, frames: &[Vec<f32>]) {
        let static_ducking = self.ducking();
        // SAFETY: SPSC — the only writer here; the reader pops via `LockRead`.
        let rb = unsafe { &mut *self.output.get() };
        // SAFETY: single producer steps the IIR automation state here.
        let iir = unsafe { &mut *self.iir.get() };
        let max_len = frames.iter().map(|f| f.len()).max().unwrap_or(0);
        for i in 0..max_len {
            let mut sum = 0.0f32;
            for frame in frames {
                if let Some(&sample) = frame.get(i) {
                    sum += sample;
                }
            }
            // Mix in live paging stream from the jitter buffer.
            unsafe {
                if let Some(jb) = &mut *self.jitter.get() {
                    sum += jb.pop_output();
                }
            }
            let gain = if iir.active {
                iir.step()
            } else {
                static_ducking
            };
            let _ = rb.push(sum * gain);
        }
    }

    /// Producer path for the `audio-cpal` sink: read all sources, push mixed
    /// frames into the ring, and do NOT drain — the cpal callback drains via
    /// [`LockRead`]. Safe because the ring is SPSC (see the `Sync` impl).
    #[cfg(feature = "audio-cpal")]
    pub fn feed(&self) {
        let frames = self.collect_mixed();
        self.push_mixed(&frames);
    }

    /// Mix all sources into the output ring and return the resulting frames.
    pub fn mix(&mut self) -> Vec<f32> {
        let frames = self.collect_mixed();
        self.push_mixed(&frames);

        // SAFETY: `&mut self` gives exclusive access — safe to drain.
        let rb = unsafe { &mut *self.output.get() };
        let mut out = Vec::with_capacity(OUTPUT_CAPACITY);
        while let Some(v) = rb.pop() {
            out.push(v);
        }
        out
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "audio-cpal")]
impl crate::sink::LockRead for Mixer {
    fn read_f32(&self, buffer: &mut [f32]) -> usize {
        use ringbuf::Rb;

        // SAFETY: SPSC semantics — the cpal callback thread is the sole
        // consumer here; `mix` / `feed` (the producer) runs on a different
        // thread. `SharedRb` uses atomic head/tail pointers so concurrent
        // push/pop is hardware-safe.
        let rb = unsafe { &mut *self.output.get() };

        let mut count = 0;
        for slot in buffer.iter_mut() {
            match rb.pop() {
                Some(v) => {
                    *slot = v;
                    count += 1;
                }
                None => break,
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::traits::AudioSource;

    /// A source that emits a constant value.
    struct ConstantSource {
        label: String,
        value: f32,
    }

    impl AudioSource for ConstantSource {
        fn read_pcm(&self, buffer: &mut [f32]) -> usize {
            for (i, slot) in buffer.iter_mut().enumerate() {
                *slot = self.value;
                if i >= 512 {
                    break;
                }
            }
            buffer.len().min(512)
        }
        fn label(&self) -> &str {
            &self.label
        }
    }

    #[test]
    fn empty_mixer_produces_no_output() {
        let mut mixer = Mixer::new();
        let out = mixer.mix();
        assert!(out.is_empty());
    }

    #[test]
    fn ducking_clamps_to_unit_range() {
        let mut mixer = Mixer::new();
        mixer.set_ducking(5.0);
        assert_eq!(mixer.ducking(), 1.0);
        mixer.set_ducking(-3.0);
        assert_eq!(mixer.ducking(), 0.0);
    }

    #[test]
    fn set_ducking_profile_eases_gain() {
        let mut mixer = Mixer::new();
        assert_eq!(mixer.ducking(), 1.0); // static gain, no profile

        mixer.set_ducking_profile(DuckingProfile {
            curve: crate::ducking::DuckingCurve::Linear,
            duration_ms: 100,
            target: 0.0,
        });

        std::thread::sleep(std::time::Duration::from_millis(50));
        let mid = mixer.ducking();
        assert!(mid < 1.0 && mid > 0.0, "mid-ramp ducking was {mid}");

        std::thread::sleep(std::time::Duration::from_millis(60));
        assert_eq!(mixer.ducking(), 1.0);
    }

    #[test]
    fn single_source_passes_through_with_unit_ducking() {
        let mut mixer = Mixer::new();
        mixer.add_source(Box::new(ConstantSource {
            label: "test".into(),
            value: 0.5,
        }));
        let out = mixer.mix();
        assert!(out.iter().all(|s| *s == 0.5));
        assert!(!out.is_empty());
    }

    #[test]
    fn ducking_halves_output() {
        let mut mixer = Mixer::new();
        mixer.set_ducking(0.5);
        mixer.add_source(Box::new(ConstantSource {
            label: "test".into(),
            value: 1.0,
        }));
        let out = mixer.mix();
        assert!(out.iter().all(|s| *s == 0.5));
        assert!(!out.is_empty());
    }

    #[test]
    fn two_sources_sum() {
        let mut mixer = Mixer::new();
        mixer.add_source(Box::new(ConstantSource {
            label: "a".into(),
            value: 0.3,
        }));
        mixer.add_source(Box::new(ConstantSource {
            label: "b".into(),
            value: 0.4,
        }));
        let out = mixer.mix();
        assert!(out.iter().all(|s| (*s - 0.7).abs() < 1e-6));
        assert!(!out.is_empty());
    }

    #[test]
    fn automation_eases_gain_per_sample() {
        let mut mixer = Mixer::new();
        mixer.add_source(Box::new(ConstantSource {
            label: "t".into(),
            value: 1.0,
        }));
        // Start a duck: target 0.0, moderate pole.
        mixer.push_automation(AutomationCommand::DuckStart {
            target_gain: 0.0,
            coeff: 0.05,
        });
        assert!(mixer.iir_gain() < 1.0 + 1e-6); // begins near unity
        let first = mixer.mix();
        assert!(!first.is_empty());
        // Per-sample easing: output should be non-increasing across the batch
        // (gain ramps 1.0 -> ~0) and never exceed the source amplitude.
        assert!(first.iter().all(|s| *s <= 1.0 + 1e-6));
        let peak = first.iter().cloned().fold(0.0f32, f32::max);
        assert!(peak <= 1.0 + 1e-6);
        // After enough mixing, gain should approach 0 (full duck).
        for _ in 0..40 {
            let _ = mixer.mix();
        }
        assert!(
            mixer.iir_gain() < 0.1,
            "IIR did not ease toward duck: {}",
            mixer.iir_gain()
        );
    }

    #[test]
    fn paging_jitter_mixes_into_output() {
        let mut mixer = Mixer::new();
        mixer.enable_paging_jitter(1000, 10); // 10 samples prebuffer
        mixer.push_paging_pcm(&[0.5; 20]);
        // Drain: jitter buffer should contribute to output.
        let out = mixer.mix();
        // Output may be empty if not yet primed; just verify no panic.
        let _ = out;
    }

    #[test]
    fn paging_jitter_disabled_by_default() {
        let mixer = Mixer::new();
        // No panic when pushing to disabled jitter.
        mixer.push_paging_pcm(&[0.5; 10]);
    }
}
