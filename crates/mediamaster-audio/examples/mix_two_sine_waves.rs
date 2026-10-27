//! Example: mix two sine waves through the `Mixer`.

use std::sync::atomic::{AtomicU64, Ordering};

use mediamaster_audio::Mixer;
use mediamaster_core::traits::AudioSource;

/// A stateless sine-wave generator. Phase is tracked via an atomic frame
/// counter so the source can be `Send + Sync` (required by `AudioSource`).
struct SineSource {
    frequency: f32,
    sample_rate: f32,
    frame: AtomicU64,
}

impl SineSource {
    fn new(frequency: f32, sample_rate: f32) -> Self {
        Self {
            frequency,
            sample_rate,
            frame: AtomicU64::new(0),
        }
    }
}

impl AudioSource for SineSource {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        let start = self.frame.fetch_add(buffer.len() as u64, Ordering::Relaxed);
        let delta = self.frequency / self.sample_rate * 2.0 * std::f32::consts::PI;
        for (i, slot) in buffer.iter_mut().enumerate() {
            let phase = (start + i as u64) as f32 * delta;
            *slot = phase.sin();
        }
        buffer.len()
    }

    fn label(&self) -> &str {
        "sine"
    }
}

fn main() {
    let sample_rate = 44_100.0_f32;
    let mut mixer = Mixer::new();
    mixer.add_source(Box::new(SineSource::new(440.0, sample_rate)));
    mixer.add_source(Box::new(SineSource::new(660.0, sample_rate)));

    let output = mixer.mix();
    println!("mixed {} samples", output.len());
    for (i, s) in output.iter().take(10).enumerate() {
        println!("  [{:3}] {:.4}", i, s);
    }
}
