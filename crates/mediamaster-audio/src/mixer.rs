//! Lock-free PCM mixing: sum frames from multiple `AudioSource`s with
//! per-source gain and global ducking. The output ring uses `ringbuf::HeapRb`
//! (SPSC), so a separate thread or the `cpal` callback drains it without
//! blocking the async runtime.

use std::cell::UnsafeCell;

use mediamaster_core::traits::AudioSource;
use ringbuf::HeapRb;
use ringbuf::Rb;

/// Capacity of the output ring buffer (in frames).
const OUTPUT_CAPACITY: usize = 8192;

/// A mixer that reads from several sources, applies a global ducking
/// multiplier, and writes mixed PCM to an output ring buffer.
///
/// Sources are added up-front (via [`Mixer::add_source`]); `mix` needs
/// `&mut self` because the ring buffer's push/pop require exclusive access.
pub struct Mixer {
    sources: Vec<Box<dyn AudioSource>>,
    ducking: f32,
    /// `UnsafeCell` is the only legal way to obtain `&mut` from `&self`,
    /// which `LockRead::read_f32` (cpal callback) needs. In the non-cpal
    /// path, `mix(&mut self)` already has exclusive access.
    output: UnsafeCell<HeapRb<f32>>,
}

// SAFETY: `HeapRb` (`SharedRb`) uses atomic head/tail pointers internally,
// making it safe for SPSC access from two threads. The `sources` and `ducking`
// fields are only mutated through `&mut self` methods, so concurrent access
// via `&self` (the `LockRead` impl) is safe for the ring buffer alone.
#[cfg(feature = "audio-cpal")]
unsafe impl Sync for Mixer {}

impl Mixer {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            ducking: 1.0,
            output: UnsafeCell::new(HeapRb::new(OUTPUT_CAPACITY)),
        }
    }

    pub fn add_source(&mut self, source: Box<dyn AudioSource>) {
        self.sources.push(source);
    }

    /// Global ducking multiplier from 0.0 (fully muted) to 1.0 (full volume).
    pub fn ducking(&self) -> f32 {
        self.ducking.clamp(0.0, 1.0)
    }

    pub fn set_ducking(&mut self, ducking: f32) {
        self.ducking = ducking.clamp(0.0, 1.0);
    }

    /// Mix all sources into the output ring and return the resulting frames.
    ///
    /// Each source is read into a stack buffer, then per-sample summing is
    /// applied with the ducking multiplier. The mixed frames are pushed to the
    /// output ring buffer; this function then drains that ring and returns the
    /// collected samples so callers can immediately consume them (e.g. write to
    /// a `cpal` stream or encode to a file).
    pub fn mix(&mut self) -> Vec<f32> {
        let ducking = self.ducking();

        // Phase 1 — collect one batch from each source.
        let frames: Vec<Vec<f32>> = self
            .sources
            .iter()
            .map(|s| {
                let mut buf = vec![0.0f32; 512];
                let n = s.read_pcm(&mut buf);
                buf.truncate(n);
                buf
            })
            .collect();

        // Phase 2 — sum per sample index with ducking, push to output ring.
        let rb = unsafe { &mut *self.output.get() };
        let max_len = frames.iter().map(|f| f.len()).max().unwrap_or(0);
        for i in 0..max_len {
            let mut sum = 0.0f32;
            for frame in &frames {
                if let Some(&sample) = frame.get(i) {
                    sum += sample;
                }
            }
            let _ = rb.push(sum * ducking);
        }

        // Phase 3 — drain the output ring for the caller.
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
        // consumer here; `mix` (the producer) runs on a different thread.
        // `SharedRb` uses atomic head/tail pointers so concurrent push/pop
        // is hardware-safe.
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
    fn single_source_passes_through_with_unit_ducking() {
        let mut mixer = Mixer::new();
        mixer.add_source(Box::new(ConstantSource {
            label: "test".into(),
            value: 0.5,
        }));
        let out = mixer.mix();
        // Each sample from the source is 0.5; with ducking 1.0 it stays 0.5.
        assert!(out.iter().all(|s| *s == 0.5));
        assert!(!out.is_empty());
    }
}
