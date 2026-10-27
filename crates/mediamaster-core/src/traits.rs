//! Async hardware traits shared across domains.


/// A source that yields PCM frames into a caller-supplied buffer.
///
/// `tv-rs` and `cctv-rs` adapters implement this by extracting PCM from their
/// respective decoding paths. The mixer depends only on this trait.
pub trait AudioSource: Send + Sync {
    /// Read up to `buffer.len()` frames into `buffer`; returns the number of
    /// frames written.
    fn read_pcm(&self, buffer: &mut [f32]) -> usize;

    /// A human-readable label, e.g. `"tv-rs ch0"` or `"cctv-mic"`.
    fn label(&self) -> &str;
}

/// A hardware sensor read through a standard async interface.
///
/// Implemented for I2C/Serial/SPI sensors wrapped in async pollers.
#[allow(async_fn_in_trait)]
pub trait AsyncSensor: Send + Sync {
    /// Return the latest reading (never blocks the async runtime).
    async fn read(&self) -> SensorReading;
}

/// One debounced sensor reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensorReading {
    MotionDetected,
    Proximity(f32),
    Light(f32),
    Occupancy,
    Open,
    Closed,
    Unknown,
}

/// A detector that scans a magnitude spectrum and finds signal peaks.
///
/// User types implement this to define what a "signal" looks like (e.g. a
/// specific carrier, burst shape, or protocol preamble). The RF pipeline calls
/// this after the FFT stage; no dependency on any SDR vendor here.
pub trait SignalDetector: Send + Sync {
    /// Return the number of peaks found, writing them into `out`.
    fn detect(&self, magnitude: &[f32], out: &mut Vec<Signal>) -> usize;

    /// The convolution threshold (only magnitudes above this count as peaks).
    fn threshold(&self) -> f32;
}

/// A detected RF signal — whatever a user's detector decides it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub id: String,
    pub confidence: f32,
    pub start_sample: usize,
    pub magnitude_peak: f32,
}
