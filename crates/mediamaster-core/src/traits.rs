//! Adapter hardware traits shared across domains.
//!
//! These traits are the **adapter surface**: `tv-rs`, `cctv-rs`, and any
//! hardware backend implement them to plug into the mediamaster ecosystem.
//! `AsyncSensor` is object-safe (`dyn AsyncSensor`) via `Pin<Box<dyn Future>>`
//! — no proc-macros, no `tokio` — so `mediamaster-sensors` can hold a
//! `Vec<Box<dyn AsyncSensor>>`.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;

// Re-export types that live in `types` so existing code that imports them
// via `mediamaster_core::traits::{Signal, SensorReading}` keeps compiling.
use crate::error::MediamasterError;
pub use crate::types::{SensorReading, Signal};

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

/// A hardware sensor read through an object-safe async interface.
///
/// Returns a boxed future so `Box<dyn AsyncSensor>` is valid. `&mut self` so a
/// driver may keep per-poll state without interior mutability.
pub trait AsyncSensor: Send + Sync {
    /// Poll the next sensor sample asynchronously.
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>>;
}

/// A detector that scans a magnitude spectrum and finds signal peaks.
///
/// User types implement this to define what a "signal" looks like (e.g. a
/// specific carrier, burst shape, or protocol preamble). The RF pipeline calls
/// this after the FFT stage; no dependency on any SDR vendor here.
pub trait SignalDetector: Send + Sync {
    /// The convolution threshold (only magnitudes above this count as peaks).
    fn threshold(&self) -> f32;

    /// Scan `magnitude`, writing detected peaks into `out`. Returns the number
    /// of peaks written.
    fn detect(&self, magnitude: &[f32], out: &mut Vec<Signal>) -> usize;
}
