//! FFT-based detection: scan the magnitude spectrum for signal peaks.

use std::sync::Arc;

use num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use mediamaster_core::traits::Signal;

/// Detector that applies an FFT and reports peak magnitudes.
pub struct MagnitudeDetector {
    threshold: f32,
    fft: Arc<dyn Fft<f32>>,
}

impl MagnitudeDetector {
    pub fn new(size: usize, threshold: f32) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(size);
        Self { threshold, fft }
    }

    /// Run the FFT over a single complex buffer and return peak magnitudes.
    pub fn peaks(&self, buf: &[Complex<f32>]) -> Vec<f32> {
        let mut work: Vec<Complex<f32>> = buf.to_vec();
        self.fft.process(&mut work);
        // Magnitude of each bin.
        work.iter().map(|c| c.norm()).collect()
    }
}

impl mediamaster_core::traits::SignalDetector for MagnitudeDetector {
    fn detect(&self, magnitude: &[f32], out: &mut Vec<Signal>) -> usize {
        let mut peaks = 0;
        for (i, m) in magnitude.iter().enumerate() {
            if *m > self.threshold {
                out.push(Signal {
                    id: i.to_string(),
                    confidence: *m,
                    start_sample: i,
                    magnitude_peak: *m,
                });
                peaks += 1;
            }
        }
        peaks
    }

    fn threshold(&self) -> f32 {
        self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::traits::SignalDetector;

    #[test]
    fn peaks_returns_non_negative_magnitudes() {
        let detector = MagnitudeDetector::new(4, 0.0);
        let input: Vec<Complex<f32>> = vec![
            Complex::new(1.0, 0.0),
            Complex::new(0.0, 1.0),
            Complex::new(1.0, 1.0),
            Complex::new(0.0, 0.0),
        ];
        let result = detector.peaks(&input);
        assert_eq!(result.len(), 4);
        assert!(result.iter().all(|&m| m >= 0.0));
    }

    #[test]
    fn detect_finds_peaks_above_threshold() {
        let detector = MagnitudeDetector::new(4, 0.5);
        let magnitudes = vec![0.1, 0.6, 0.3, 0.8];
        let mut out: Vec<Signal> = Vec::new();
        let count = detector.detect(&magnitudes, &mut out);
        assert_eq!(count, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "1");
        assert_eq!(out[1].id, "3");
    }

    #[test]
    fn threshold_returns_configured_value() {
        let detector = MagnitudeDetector::new(4, 0.5);
        assert_eq!(detector.threshold(), 0.5);
    }
}
