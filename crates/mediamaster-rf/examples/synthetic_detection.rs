//! Example: FFT-based signal detection on synthetic IQ data.

use mediamaster_core::traits::SignalDetector;
use mediamaster_rf::{IqSample, MagnitudeDetector};

fn main() {
    let detector = MagnitudeDetector::new(8, 0.5);

    // Synthetic IQ: tones at bins 1 and 4, silence elsewhere.
    let iq: Vec<IqSample> = (0..8)
        .map(|i| {
            if i == 1 || i == 4 {
                IqSample::new(1.0, 0.0)
            } else {
                IqSample::new(0.0, 0.0)
            }
        })
        .collect();

    let mags = detector.peaks(&iq);
    println!("FFT magnitudes ({} bins):", mags.len());
    for (i, m) in mags.iter().enumerate() {
        println!("  bin[{:3}] = {:.4}", i, m);
    }

    let mut signals = Vec::new();
    let n = detector.detect(&mags, &mut signals);
    println!("detected {} peaks above threshold 0.5", n);
    for s in &signals {
        println!(
            "  Signal id={} bin={} mag={:.4} conf={:.4}",
            s.id, s.start_sample, s.magnitude_peak, s.confidence
        );
    }

    println!("\nthreshold: {}", detector.threshold());
}
