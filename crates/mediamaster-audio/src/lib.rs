//! Lock-free PCM mixing, ducking, and routing for the mediamaster-rs ecosystem.
//!
//! This crate mixes PCM frames from multiple [`AudioSource`] implementations,
//! applies per-source gain and global ducking, and routes the result to an OS
//! audio sink via `cpal`.
//!
//! Default features are empty — enable `audio-cpal` for the OS sink and `decode`
//! for Symphonia-based asset decoding.

pub mod ducking;
pub mod mixer;
pub mod sink;
pub mod source;
pub mod zones;

pub use ducking::{DuckingCurve, DuckingProfile};
pub use zones::{Zone, ZoneId, ZoneRouter, Zones};
pub use mixer::Mixer;
#[cfg(feature = "audio-cpal")]
pub use sink::AudioSink;
pub use sink::LockRead;
pub use source::MediaSourceAdapter;
