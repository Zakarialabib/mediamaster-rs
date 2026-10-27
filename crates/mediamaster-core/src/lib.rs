//! The shared vocabulary of the mediamaster-rs ecosystem.
//!
//! All types, traits, and the event bus are re-exported at the crate root so
//! downstream users can write:
//!
//! ```rust
//! use mediamaster_core::{
//!     EventBus, HardwareEvent, RoomState, Signal, SensorReading, SignalDetector,
//! };
//! ```
//!
//! …instead of `mediamaster_core::events::EventBus`.

pub mod error;
pub mod events;
pub mod traits;
pub mod types;

pub use error::MediamasterError;
pub use events::{EventBus, HardwareEvent};
pub use traits::{AsyncSensor, AudioSource, SensorReading, Signal, SignalDetector};
pub use types::{AudioTarget, BoundingBox, RadioBurst, RoomState};
