//! RF domain bridge: adapter and device enumerators.
//!
//! This module bridges the `mediamaster-core::SignalDetector` trait to the
//! `mediamaster-rf` domain crate, and provides device enumerators for
//! internal and external RF hardware.
//!
//! # Usage
//!
//! ```no_run
//! use mediamaster_bridge::RfBridge;
//!
//! let bridge = RfBridge::new();
//! let devices = bridge.enumerate_internal_rf_devices();
//! # let _ = devices;
//! ```

use mediamaster_core::traits::SignalDetector;
use mediamaster_core::types::Signal;
use mediamaster_rf::detector::MagnitudeDetector;

/// Default FFT size for the bridge's detector (the apps build their own
/// `MagnitudeDetector` when they need a different size).
const DEFAULT_FFT_SIZE: usize = 1024;
/// Default convolution threshold: only magnitudes above this count as peaks.
const DEFAULT_THRESHOLD: f32 = 0.5;

/// Bridge from `mediamaster-core::SignalDetector` → `mediamaster-rf`.
///
/// `RfBridge` wraps a `MagnitudeDetector` and provides the `SignalDetector`
/// trait implementation for the applications.
pub struct RfBridge {
    detector: MagnitudeDetector,
}

impl RfBridge {
    /// Create a new RF bridge with a default detector.
    pub fn new() -> Self {
        Self {
            detector: MagnitudeDetector::new(DEFAULT_FFT_SIZE, DEFAULT_THRESHOLD),
        }
    }

    /// Enumerate internal RF devices.
    ///
    /// Internal RF devices are device-embedded SDR chips (e.g., RTL-SDR found
    /// on development boards).
    pub fn enumerate_internal_rf_devices(&self) -> Vec<String> {
        // Placeholder: applications enumerate via hardware APIs.
        vec!["RTL-SDR (embedded)".to_string()]
    }

    /// Enumerate external RF devices.
    ///
    /// External devices include:
    /// - USB SDR dongles (RTL-SDR, HackRF)
    /// - External SDR receivers
    pub fn enumerate_external_rf_devices(&self) -> Vec<String> {
        vec![
            "RTL-SDR (USB)".to_string(),
            "HackRF (USB)".to_string(),
            "External SDR receiver".to_string(),
        ]
    }

    /// Get the underlying detector (for direct access by the application).
    pub fn detector(&self) -> &MagnitudeDetector {
        &self.detector
    }
}

impl Default for RfBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalDetector for RfBridge {
    fn threshold(&self) -> f32 {
        self.detector.threshold()
    }

    fn detect(&self, magnitude: &[f32], out: &mut Vec<Signal>) -> usize {
        self.detector.detect(magnitude, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_threshold_matches_detector() {
        let bridge = RfBridge::new();
        assert_eq!(bridge.threshold(), DEFAULT_THRESHOLD);
    }

    #[test]
    fn detect_delegates_to_detector() {
        let bridge = RfBridge::new();
        let magnitudes = [0.1, 0.9, 0.2, 0.8];
        let mut out: Vec<Signal> = Vec::new();
        let count = bridge.detect(&magnitudes, &mut out);
        assert_eq!(count, 2);
        assert_eq!(out.len(), 2);
    }
}
