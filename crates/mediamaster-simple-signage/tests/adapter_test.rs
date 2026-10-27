//! Integration tests for the Simple-Signage ↔ mediamaster-rs bridge.

use mediamaster_simple_signage::{
    AudioEngine, AudioSource, SimpleSignageAudioSource, SimpleSignageAudioStream,
};

/// A deterministic signage stream used to validate the adapter + engine.
struct ConstantStream {
    frames: Vec<f32>,
    label: String,
    priority: bool,
}

impl SimpleSignageAudioStream for ConstantStream {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        let n = self.frames.len().min(buffer.len());
        buffer[..n].copy_from_slice(&self.frames[..n]);
        n
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn is_priority(&self) -> bool {
        self.priority
    }
}

fn constant(value: f32, label: &str) -> ConstantStream {
    ConstantStream {
        frames: vec![value; 512],
        label: label.into(),
        priority: false,
    }
}

#[test]
fn adapter_implements_audiosource_and_surfaces_label() {
    let stream = constant(0.2, "chime");
    let source = SimpleSignageAudioSource::new(stream, "chime");
    assert_eq!(source.label(), "chime");
    let mut buf = vec![0.0f32; 16];
    let n = source.read_pcm(&mut buf);
    assert_eq!(n, 16);
    assert!(buf.iter().all(|s| *s == 0.2));
}

#[test]
fn engine_mixes_multiple_sources() {
    let mut engine = AudioEngine::new();
    engine.add_signage_source(SimpleSignageAudioSource::new(constant(0.3, "a"), "a"));
    engine.add_signage_source(SimpleSignageAudioSource::new(constant(0.4, "b"), "b"));
    let out = engine.mix_step();
    assert!(!out.is_empty());
    // 0.3 + 0.4 = 0.7 with unit ducking.
    assert!(out.iter().all(|s| (*s - 0.7).abs() < 1e-6));
}

#[test]
fn engine_ducking_scales_output() {
    let mut engine = AudioEngine::new();
    engine.add_signage_source(SimpleSignageAudioSource::new(
        constant(1.0, "alert"),
        "alert",
    ));
    engine.set_ducking(0.25);
    let out = engine.mix_step();
    assert!(out.iter().all(|s| (*s - 0.25).abs() < 1e-6));
}

#[test]
fn engine_exposes_ducking_bounds() {
    let mut engine = AudioEngine::new();
    engine.set_ducking(5.0);
    assert_eq!(engine.ducking(), 1.0);
    engine.set_ducking(-1.0);
    assert_eq!(engine.ducking(), 0.0);
}

#[test]
fn empty_engine_produces_no_output() {
    let mut engine = AudioEngine::new();
    assert!(engine.mix_step().is_empty());
}
