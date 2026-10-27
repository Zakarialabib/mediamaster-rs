//! Integration test: MagnitudeDetector (rf) implements SignalDetector (core).

use mediamaster_core::traits::{Signal, SignalDetector};
use mediamaster_rf::MagnitudeDetector;

#[test]
fn detector_finds_bin_energy_above_threshold() {
    let detector = MagnitudeDetector::new(8, 1.0);
    let magnitudes = vec![0.1, 2.0, 0.5, 3.0, 0.1, 0.5, 0.5, 1.5];

    let mut signals: Vec<Signal> = Vec::new();
    let count = detector.detect(&magnitudes, &mut signals);

    assert_eq!(count, 3);
    assert_eq!(signals[0].id, "1");
    assert_eq!(signals[1].id, "3");
    assert_eq!(signals[2].id, "7");
}

#[test]
fn threshold_method_returns_configured_value() {
    let detector = MagnitudeDetector::new(8, 2.5);
    assert_eq!(detector.threshold(), 2.5);
}
