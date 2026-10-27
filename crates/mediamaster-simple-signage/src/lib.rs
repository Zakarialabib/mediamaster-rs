//! SimpleSignage adapter for the mediamaster-rs ecosystem.
//!
//! Bridges signage-application audio streams into the mediamaster-rs mixing
//! engine.  This crate is the **library-side seam** described in
//! `MIGRATION.md` §3 — it lives inside the `mediamaster-rs` workspace but
//! speaks the SimpleSignage vocabulary (`SimpleSignageAudioStream`) and adapts
//! it to the engine's core trait (`AudioSource`).
//!
//! ## Usage
//!
//! ```no_run
//! use mediamaster_simple_signage::{AudioEngine, SimpleSignageAudioSource, SimpleSignageAudioStream};
//!
//! struct MyAlertSound;
//! impl SimpleSignageAudioStream for MyAlertSound {
//!     fn read_pcm(&self, buf: &mut [f32]) -> usize {
//!         buf.fill(0.5); buf.len()
//!     }
//!     fn label(&self) -> &str { "alert" }
//!     fn is_priority(&self) -> bool { true }
//! }
//!
//! let mut engine = AudioEngine::new();
//! engine.add_signage_source(SimpleSignageAudioSource::new(MyAlertSound, "alert"));
//! let _mixed = engine.mix_step(); // one batch of mixed PCM
//! ```
//!
//! Enable the `audio-cpal` feature to hand the mixer off to the OS audio
//! thread instead of calling `mix_step` manually.

// Re-export core types for downstream convenience — downstream crates can
// `use mediamaster_simple_signage::{AudioEngine, EventBus, HardwareEvent}`
// without pulling in `mediamaster_core` separately.
pub use mediamaster_core::traits::AudioSource;
pub use mediamaster_core::{EventBus, HardwareEvent};

use mediamaster_audio::Mixer;
use tokio::sync::broadcast::Receiver;

// ---------------------------------------------------------------------------
// App-side contract
// ---------------------------------------------------------------------------

/// The signage application's PCM stream contract.
///
/// A signage app (Tauri plugin, launcher, etc.) implements this for each
/// audio source it wants to route through the mediamaster engine.
/// The adapter wraps it and translates to the core [`AudioSource`] trait.
pub trait SimpleSignageAudioStream: Send + Sync {
    /// Read up to `buffer.len()` normalized PCM frames (-1.0..=1.0) into
    /// `buffer`.  Returns the number of frames written.
    fn read_pcm(&self, buffer: &mut [f32]) -> usize;

    /// Human-readable source label for debugging / UI.
    fn label(&self) -> &str;

    /// `true` if this source is high-priority (alerts, doorbell, TTS).
    /// The engine may route priority sources ahead of background audio.
    fn is_priority(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Core-trait adapter
// ---------------------------------------------------------------------------

/// Adapts any [`SimpleSignageAudioStream`] to the mediamaster [`AudioSource`]
/// trait so it can be fed to [`Mixer`].
///
/// The `priority` flag is preserved for the engine to use when ordering
/// sources inside the mix.
pub struct SimpleSignageAudioSource {
    inner: Box<dyn SimpleSignageAudioStream>,
    priority: bool,
    label: String,
}

impl SimpleSignageAudioSource {
    /// Wrap any `SimpleSignageAudioStream` in a core `AudioSource` adapter.
    pub fn new(stream: impl SimpleSignageAudioStream + 'static, label: impl Into<String>) -> Self {
        let priority = stream.is_priority();
        Self {
            inner: Box::new(stream),
            priority,
            label: label.into(),
        }
    }

    /// Whether this source was marked high-priority at construction time.
    pub fn is_priority(&self) -> bool {
        self.priority
    }
}

impl AudioSource for SimpleSignageAudioSource {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        self.inner.read_pcm(buffer)
    }

    fn label(&self) -> &str {
        &self.label
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// High-level audio engine for signage applications.
///
/// Owns a [`Mixer`] and provides convenience methods for adding signage
/// streams and configuring ducking.  Call [`mix_step`](Self::mix_step) in a
/// polling loop; it automatically drains pending [`HardwareEvent`]s (e.g.
/// [`AudioDuckRequest`](HardwareEvent::AudioDuckRequest)) before mixing.
pub struct AudioEngine {
    mixer: Mixer,
    receiver: Option<Receiver<HardwareEvent>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            mixer: Mixer::new(),
            receiver: None,
        }
    }

    /// Add a pre-built `AudioSource` (e.g. a tv-rs/cctv-rs adapter).
    pub fn add_source(&mut self, source: impl AudioSource + 'static) {
        self.mixer.add_source(Box::new(source));
    }

    /// Add a signage source wrapped in the `SimpleSignageAudioSource` adapter.
    pub fn add_signage_source(&mut self, source: impl AudioSource + 'static) {
        self.mixer.add_source(Box::new(source));
    }

    /// Convenience: wrap a raw `SimpleSignageAudioStream` and add it.
    pub fn signage_stream(
        &mut self,
        stream: impl SimpleSignageAudioStream + 'static,
        label: impl Into<String>,
    ) {
        self.add_signage_source(SimpleSignageAudioSource::new(stream, label));
    }

    /// Set the global ducking level (0.0 = muted, 1.0 = full volume).
    pub fn set_ducking(&mut self, level: f32) {
        self.mixer.set_ducking(level);
    }

    /// Current ducking level (clamped to 0.0..=1.0).
    pub fn ducking(&self) -> f32 {
        self.mixer.ducking()
    }

    /// Mix one batch of PCM frames from all registered sources.
    ///
    /// Drains any pending `HardwareEvent`s first (duck requests, etc.).
    pub fn mix_step(&mut self) -> Vec<f32> {
        self.process_events();
        self.mixer.mix()
    }

    /// Subscribe to a core `EventBus` for automatic ducking updates.
    pub fn subscribe(&mut self, bus: &EventBus) {
        self.receiver = Some(bus.subscribe());
    }

    /// Drain pending events and apply side-effects (e.g. `AudioDuckRequest`).
    pub fn process_events(&mut self) {
        if let Some(rx) = &mut self.receiver {
            while let Ok(event) = rx.try_recv() {
                if let HardwareEvent::AudioDuckRequest { ducking, .. } = event {
                    self.mixer.set_ducking(ducking);
                }
            }
        }
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "audio-cpal")]
impl AudioEngine {
    /// Hand the mixer off to a cpal output stream.
    ///
    /// Consumes `self` — after calling this, use the returned stream to
    /// control playback.  The mixer runs on the cpal audio thread; the
    /// engine can no longer call `mix_step`.
    pub fn into_cpal_sink(self) -> Result<cpal::Stream, mediamaster_core::MediamasterError> {
        use mediamaster_audio::AudioSink;
        use std::sync::Arc;

        let arc_mixer = Arc::new(self.mixer);
        let sink = AudioSink::open_default().map_err(mediamaster_core::MediamasterError::other)?;
        let stream = sink
            .start::<Mixer>(arc_mixer)
            .map_err(mediamaster_core::MediamasterError::other)?;
        Ok(stream)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct SilenceStream;
    impl SimpleSignageAudioStream for SilenceStream {
        fn read_pcm(&self, buffer: &mut [f32]) -> usize {
            for slot in buffer.iter_mut() {
                *slot = 0.0;
            }
            buffer.len()
        }
        fn label(&self) -> &str {
            "silence"
        }
    }

    #[test]
    fn adapter_read_pcm_and_label() {
        let source = SimpleSignageAudioSource::new(SilenceStream, "chime");
        assert_eq!(source.label(), "chime");
        assert!(!source.is_priority());

        let mut buf = vec![0.0f32; 8];
        let n = source.read_pcm(&mut buf);
        assert_eq!(n, 8);
        assert!(buf.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn engine_new_is_empty() {
        let mut engine = AudioEngine::new();
        assert_eq!(engine.ducking(), 1.0);
        assert!(engine.mix_step().is_empty());
    }

    #[test]
    fn engine_ducking_clamps() {
        let mut engine = AudioEngine::new();
        engine.set_ducking(5.0);
        assert_eq!(engine.ducking(), 1.0);
        engine.set_ducking(-1.0);
        assert_eq!(engine.ducking(), 0.0);
    }

    #[test]
    fn process_events_noop_without_subscription() {
        let mut engine = AudioEngine::new();
        engine.process_events(); // must not panic
    }
}
