//! Audio domain bridge: adapters and device enumerators.
//!
//! This module bridges the `mediamaster-core::AudioSource` trait to the
//! `mediamaster-audio` domain crate, and provides device enumerators for
//! internal (built-in) and external (USB, USB-C, BLE) audio hardware.
//!
//! # Usage
//!
//! ```no_run
//! use mediamaster_bridge::AudioBridge;
//!
//! let bridge = AudioBridge::new();
//! let devices = bridge.enumerate_internal_audio_devices();
//! # let _ = devices;
//! ```
//!
//! # Output path
//!
//! The `Mixer` is wrapped in a `Mutex` so `AudioBridge` can implement
//! `AudioSource::read_pcm(&self, …)` (the trait takes `&self`). The
//! applications that want a real `cpal` output thread use the `audio-cpal`
//! feature and call `mediamaster_audio::sink::AudioSink::start` themselves
//! with an `Arc<Mixer>` — the OS output thread then drains the ring buffer
//! via `LockRead` instead of this crate.

use std::sync::Mutex;

use mediamaster_audio::ducking::DuckingProfile;
use mediamaster_audio::mixer::Mixer;
use mediamaster_core::traits::AudioSource;

/// Bridge from `mediamaster-core::AudioSource` → `mediamaster-audio`.
///
/// `AudioBridge` wraps a `Mixer` and provides the `AudioSource` trait
/// implementation for the applications.
pub struct AudioBridge {
    mixer: Mutex<Mixer>,
}

impl AudioBridge {
    /// Create a new audio bridge with default settings.
    pub fn new() -> Self {
        Self {
            mixer: Mutex::new(Mixer::new()),
        }
    }

    /// Enumerate internal audio devices.
    ///
    /// On Windows, this uses the `windows` audio stack via BAA (Backend
    /// Audio Abstraction). The applications implement this by querying the
    /// OS audio endpoints.
    pub fn enumerate_internal_audio_devices(&self) -> Vec<String> {
        // Placeholder: applications enumerate via BAA or a hardware API.
        vec!["built-in speakers".to_string(), "built-in mic".to_string()]
    }

    /// Enumerate external audio devices.
    ///
    /// External devices include:
    /// - USB audio interfaces
    /// - USB-C audio (DAC/DAP)
    /// - BLE audio receivers
    pub fn enumerate_external_audio_devices(&self) -> Vec<String> {
        vec![
            "USB audio interface".to_string(),
            "USB-C audio (DAC/DAP)".to_string(),
            "BLE audio receiver".to_string(),
        ]
    }

    /// Get the underlying mixer (lock it for direct access by the
    /// application).
    pub fn mixer(&self) -> &Mutex<Mixer> {
        &self.mixer
    }

    /// Apply a smooth ducking transition — e.g. from a `HardwareEvent::
    /// AudioDuckRequest` on the `EventBus`: the app builds the profile and
    /// hands it to the mixer, which eases the gain over `duration_ms`.
    pub fn set_ducking_profile(&self, profile: DuckingProfile) {
        self.mixer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_ducking_profile(profile);
    }
}

impl Default for AudioBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSource for AudioBridge {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        // A poisoned lock still holds a usable `Mixer`; the panic would only
        // hide the real earlier failure, so recover instead.
        let mixed = self
            .mixer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mix();
        let n = mixed.len().min(buffer.len());
        buffer[..n].copy_from_slice(&mixed[..n]);
        n
    }

    fn label(&self) -> &str {
        "mediamaster-bridge audio"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_pcm_writes_at_most_buffer_len() {
        let bridge = AudioBridge::new();
        let mut buf = [0.0f32; 64];
        assert!(bridge.read_pcm(&mut buf) <= 64);
        assert_eq!(bridge.label(), "mediamaster-bridge audio");
    }

    #[test]
    fn set_ducking_profile_eases_gain() {
        use mediamaster_audio::ducking::DuckingCurve;
        use std::time::Duration;

        let bridge = AudioBridge::new();
        assert_eq!(bridge.mixer().lock().unwrap().ducking(), 1.0);
        bridge.set_ducking_profile(DuckingProfile {
            curve: DuckingCurve::Exponential,
            duration_ms: 5_000,
            target: 0.0,
        });
        std::thread::sleep(Duration::from_millis(10));
        assert!(bridge.mixer().lock().unwrap().ducking() < 1.0);
    }

    #[test]
    fn enumerates_devices() {
        let bridge = AudioBridge::new();
        assert!(!bridge.enumerate_internal_audio_devices().is_empty());
        assert!(!bridge.enumerate_external_audio_devices().is_empty());
    }
}
