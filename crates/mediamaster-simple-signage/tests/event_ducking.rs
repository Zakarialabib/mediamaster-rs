//! EventBus-driven ducking (MediaScope §6.4 `AudioDuckRequest`) end-to-end.
//!
//! The engine subscribes to the core `EventBus`; `AudioDuckRequest` events
//! duck the mix on the next `mix_step` (sync `try_iter` — no async runtime).

use mediamaster_simple_signage::{
    AudioEngine, EventBus, HardwareEvent, SimpleSignageAudioSource, SimpleSignageAudioStream,
};

/// A source that repeats a constant sample value.
struct Constant(f32);

impl SimpleSignageAudioStream for Constant {
    fn read_pcm(&self, buf: &mut [f32]) -> usize {
        let n = buf.len().min(512);
        for slot in buf[..n].iter_mut() {
            *slot = self.0;
        }
        n
    }
    fn label(&self) -> &str {
        "constant"
    }
}

#[test]
fn audio_duck_request_ducks_mix() {
    let mut engine = AudioEngine::new();
    engine.add_signage_source(SimpleSignageAudioSource::new(Constant(1.0), "bg"));
    let bus = EventBus::new(16);
    engine.subscribe(&bus);

    // Baseline: full volume (ducking 1.0, source 1.0 → 1.0).
    let full = engine.mix_step();
    assert!(full.iter().all(|s| (*s - 1.0).abs() < 1e-6));

    // App publishes a duck request; the next mix step reflects it.
    assert!(bus.publish(HardwareEvent::AudioDuckRequest {
        source: "camera".into(),
        ducking: 0.2,
        until: 0,
    }));
    let ducked = engine.mix_step();
    assert!(ducked.iter().all(|s| (*s - 0.2).abs() < 1e-6));
    assert_eq!(engine.ducking(), 0.2);
}

#[test]
fn mix_step_drains_pending_events() {
    let mut engine = AudioEngine::new();
    let bus = EventBus::new(16);
    engine.subscribe(&bus);

    // First mix step drains no events (nothing published yet).
    let _ = engine.mix_step();

    // Publish two duck requests in sequence; the last wins.
    assert!(bus.publish(HardwareEvent::AudioDuckRequest {
        source: "a".into(),
        ducking: 0.8,
        until: 0,
    }));
    assert!(bus.publish(HardwareEvent::AudioDuckRequest {
        source: "b".into(),
        ducking: 0.1,
        until: 0,
    }));
    let _ = engine.mix_step();
    assert_eq!(engine.ducking(), 0.1);
}

#[test]
fn no_subscription_is_safe_noop() {
    let mut engine = AudioEngine::new();
    // Not subscribed → process_events must not panic.
    engine.process_events();
    assert!(engine.mix_step().is_empty());
}
