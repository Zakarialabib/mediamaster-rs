//! Sensor abstraction and context fusion.
//!
//! `mediamaster-sensors` provides debouncing, aggregation, and fusion on top of
//! the `AsyncSensor` trait defined in `mediamaster-core`.

pub mod aggregator;
pub mod debouncer;
pub mod fusion;
pub mod sensor;

pub use aggregator::SensorAggregator;
pub use debouncer::DebouncedSensor;
pub use fusion::FusionEngine;
pub use sensor::AsyncSensor;
