//! `mediamaster-agent` — autonomous reconciliation loop uniting audio ducking,
//! context fusion, and RF signal presence.
//!
//! The agent is a **live coordination loop**: it subscribes to the shared
//! `EventBus` and reacts to the protocol the whole ecosystem speaks — on
//! `RoomState::EmergencyOverride` it commands the audio mixer to drop to
//! `FULL_MUTE` for `duck_duration_ms` and emits an audit `AudioDuckRequest`;
//! on a live `RadioBurstDetected` (paging/protocol burst) it marks the stream
//! active and ducks.
//!
//! The host drives each engine (`mediamaster-audio`, `mediamaster-sensors`,
//! `mediamaster-rf`) and wires its output into the same `EventBus`; this agent
//! only subscribes, so it is runtime-agnostic and trivially testable.

mod agent;

pub use agent::{Agent, AgentError};

pub use mediamaster_audio::ducking::{DuckingCurve, DuckingProfile};
pub use mediamaster_bus::EventBus;
pub use mediamaster_core::{
    events::{self, HardwareEvent},
    EventPublisher, RoomState,
};
pub use mediamaster_rf::Pipeline;
pub use mediamaster_sensors::FusionEngine;
