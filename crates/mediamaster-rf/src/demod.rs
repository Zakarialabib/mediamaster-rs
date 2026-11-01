//! OOK/ASK envelope demodulator.
//!
//! Recovers a byte stream from raw I/Q samples for on-off keying (OOK) /
//! amplitude-shift keying (ASK) — the modulation used by the common 315/433 MHz
//! ISM-band devices (garage doors, doorbells, remotes, weather sensors).

use crate::iq_source::IqSample;

/// On-off keying (OOK) / amplitude-shift keying (ASK) envelope demodulator.
///
/// The standard demodulation path for 315/433 MHz ISM-band devices. It recovers
/// a bit stream from raw I/Q by:
///
/// 1. Computing the envelope (magnitude) of each sample,
/// 2. Averaging the envelope over `samples_per_symbol` consecutive samples to
///    integrate over one symbol period (noise-robust),
/// 3. Slicing the averaged envelope against `threshold` to recover a bit
///    (carrier present → `1`, absent → `0`),
/// 4. Packing the bits most-significant-bit first into bytes.
///
/// A real SDR front-end (RTL-SDR, SoapySDR) feeds its I/Q stream through this
/// to produce the byte stream that a [`ProtocolParser`](crate::parser::ProtocolParser)
/// then scans for a sync word.
pub struct OokDemodulator {
    threshold: f32,
    samples_per_symbol: usize,
}

impl OokDemodulator {
    /// Create a demodulator.
    ///
    /// - `threshold`: envelope level above which a symbol counts as a `1`
    ///   (normalized amplitude, typically ~0.3–0.5 of full scale).
    /// - `samples_per_symbol`: how many I/Q samples make up one bit; must match
    ///   the transmitter's symbol rate relative to the SDR sample rate. Values
    ///   `< 1` are clamped to `1`.
    pub fn new(threshold: f32, samples_per_symbol: usize) -> Self {
        Self {
            threshold,
            samples_per_symbol: samples_per_symbol.max(1),
        }
    }

    /// Envelope level above which a symbol is a `1`.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// I/Q samples integrated per bit decision.
    pub fn samples_per_symbol(&self) -> usize {
        self.samples_per_symbol
    }

    /// Demodulate a batch of I/Q samples into a byte stream (MSB-first).
    ///
    /// A trailing group of fewer than 8 bits is emitted as a byte with its low
    /// bits zero-padded; protocol payloads are byte-aligned, so in practice the
    /// bit count is a multiple of 8.
    pub fn demodulate(&self, samples: &[IqSample]) -> Vec<u8> {
        if samples.is_empty() {
            return Vec::new();
        }
        let bits: Vec<bool> = samples
            .chunks(self.samples_per_symbol)
            .map(|window| {
                let envelope = window.iter().map(|s| s.norm()).sum::<f32>() / window.len() as f32;
                envelope > self.threshold
            })
            .collect();

        let mut out = Vec::with_capacity(bits.len().div_ceil(8));
        for group in bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &bit) in group.iter().enumerate() {
                if bit {
                    byte |= 1 << (7 - i);
                }
            }
            out.push(byte);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex;

    /// Encode bytes as OOK I/Q: bit 1 → full-scale carrier, bit 0 → silence,
    /// each bit spanning `sps` samples.
    fn encode(bytes: &[u8], sps: usize) -> Vec<IqSample> {
        let mut out = Vec::new();
        for &b in bytes {
            for i in (0..8).rev() {
                let on = (b >> i) & 1 == 1;
                let amp = if on { 1.0 } else { 0.0 };
                for _ in 0..sps {
                    out.push(Complex::new(amp, 0.0));
                }
            }
        }
        out
    }

    #[test]
    fn demodulate_recovers_bytes() {
        let demod = OokDemodulator::new(0.5, 4);
        let samples = encode(&[0xaa, 0xbb, 0x01, 0x02], 4);
        assert_eq!(demod.demodulate(&samples), vec![0xaa, 0xbb, 0x01, 0x02]);
    }

    #[test]
    fn demodulate_empty_input_is_empty() {
        let demod = OokDemodulator::new(0.5, 4);
        assert!(demod.demodulate(&[]).is_empty());
    }

    #[test]
    fn zero_samples_per_symbol_clamps_to_one() {
        let demod = OokDemodulator::new(0.5, 0);
        assert_eq!(demod.samples_per_symbol(), 1);
    }

    #[test]
    fn threshold_and_sps_accessors() {
        let demod = OokDemodulator::new(0.3, 8);
        assert_eq!(demod.threshold(), 0.3);
        assert_eq!(demod.samples_per_symbol(), 8);
    }

    #[test]
    fn demodulate_ignores_noise_below_threshold() {
        // Every sample is a weak 0.1 carrier — below a 0.5 threshold → all 0.
        let samples: Vec<IqSample> = (0..16).map(|_| Complex::new(0.1, 0.0)).collect();
        let demod = OokDemodulator::new(0.5, 4);
        assert_eq!(demod.demodulate(&samples), vec![0x00]);
    }
}
