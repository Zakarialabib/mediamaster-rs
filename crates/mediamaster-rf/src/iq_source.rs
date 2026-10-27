//! I/Q source trait: producers of raw in-phase/quadrature samples.

use num_complex::Complex;
#[allow(async_fn_in_trait)]
/// A source of complex I/Q samples. Implemented for RTL-SDR, SoapySDR, and
/// synthetic test generators.
pub trait IqSource: Send + Sync {
    /// Acquire the next batch of I/Q samples.
    async fn acquire(&self, count: usize) -> Vec<Complex<f32>>;

    /// Desired sampling rate in Hz.
    fn sample_rate_hz(&self) -> u32;
}

/// Convenience alias for a complex I/Q sample.
pub type IqSample = Complex<f32>;
