//! The shared vocabulary of the mediamaster-rs ecosystem.
//!
//! `mediamaster-core` is **`alloc`-only + `tokio`-free**: it carries only plain
//! types, object-safe traits, and the capability vocabulary, so real-time
//! audio/DSP threads and WASM/mobile consumers never link an async runtime just
//! to share vocabulary. Normal builds are `no_std`; the `#[cfg(test)]` harness
//! re-enables `std` so unit tests can use the standard prelude.
//!
//! The concrete event bus lives in `mediamaster-bus` (tokio) and implements the
//! [`EventPublisher`] trait defined here.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod caps;
pub mod error;
pub mod events;
pub mod provisioning;
pub mod traits;
pub mod types;

pub use caps::{
    CapsMatrix, Codec, CodecCaps, Confidence, DegradationTier, ObservedCodecCaps, PixelFormat,
};
pub use error::MediamasterError;
pub use events::{EventPublisher, HardwareEvent};
pub use provisioning::{MaintenanceLease, MediaCapabilityBundleV1};
pub use traits::{AsyncSensor, AudioSource, SensorReading, Signal, SignalDetector};
pub use types::{AudioTarget, BoundingBox, RadioBurst, RoomState};
