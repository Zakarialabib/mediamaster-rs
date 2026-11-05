//! Lock-free PCM mixing, ducking, and routing for the mediamaster-rs ecosystem.
//!
//! This crate mixes PCM frames from multiple [`MediaSourceAdapter`] implementations,
//! applies per-source gain and global ducking, and routes the result to an OS
//! audio sink via `cpal`.
//!
//! Default features are empty — enable `audio-cpal` for the OS sink and `decode`
//! for Symphonia-based asset decoding.

pub mod ducking;
pub mod jitter;
pub mod mixer;
pub mod opus_source;
pub mod sink;
pub mod source;
pub mod zones;

pub use ducking::{AutomationCommand, DuckingCurve, DuckingProfile, IirSmoothing};
pub use jitter::{PagingJitterBuffer, PagingSource};
pub use mixer::Mixer;
pub use opus_source::{OpusError, OpusPagingSource, OPUS_FRAME_SAMPLES_20MS, OPUS_SAMPLE_RATE};
#[cfg(feature = "audio-cpal")]
pub use sink::AudioSink;
pub use sink::LockRead;
#[cfg(feature = "decode")]
pub use source::PcmFileSource;
pub use source::{CallbackSource, MediaSourceAdapter, ReshapingSource};
pub use zones::{Zone, ZoneId, ZoneRouter, Zones};
