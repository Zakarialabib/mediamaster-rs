//! Async sensor trait wrapper.
//!
//! Re-exports the `AsyncSensor` trait from `mediamaster-core` so consumers of
//! this crate can depend solely on `mediamaster-sensors` for the full sensor
//! abstraction surface.

pub use mediamaster_core::traits::AsyncSensor;
