//! Mediamaster Bridge — unified API for mediamaster-rs domains.
//!
//! This crate is the **bridge** between the `mediamaster-rs` library crates and
//! the external applications (Windows Tauri desktop player, Android phone
//! companion app). It provides:
//!
//! - **Facade**: re-exports domain crates with a unified API
//! - **Trait adapters**: implementations of `mediamaster-core` traits wired to
//!   the domain crates
//! - **Device enumerators**: enumerate available audio interfaces, sensors,
//!   and SDR devices (internal + external)
//! - **Event handlers**: react to `EventBus` events (`AudioDuckRequest`,
//!   `SensorContextChange`, `RadioBurstDetected`)
//!
//! # Architecture
//!
//! - **Domain sub-modules**: `audio`, `sensors`, `rf` — each provides adapter
//!   implementations for that domain
//! - **Device sub-module**: `device` — device enumerators and event handlers
//! - **Facade**: `mediamaster_bridge::AudioBridge`, `SensorBridge`,
//!   `RfBridge` — unified entry points for the applications
//!
//! # Example
//!
//! ```no_run
//! use mediamaster_bridge::{AudioBridge, RfBridge, SensorBridge};
//! use mediamaster_core::traits::{AudioSource, SignalDetector};
//!
//! // 1. Build bridges (map core traits → domain crates)
//! let audio_bridge = AudioBridge::new();
//! let sensor_bridge = SensorBridge::new();
//! let rf_bridge = RfBridge::new();
//!
//! // 2. Use the bridges as AudioSource / SignalDetector
//! let _ = audio_bridge.read_pcm(&mut [0.0; 1024]);
//! let _ = rf_bridge.threshold();
//!
//! // 3. Enumerate devices
//! let _internal_audio = audio_bridge.enumerate_internal_audio_devices();
//! let _external_rf = rf_bridge.enumerate_external_rf_devices();
//!
//! // 4. Direct domain access (fusion engine, mixer) for the applications.
//! let _ = sensor_bridge.fusion();
//! ```

pub mod audio;
pub mod device;
pub mod events;
pub mod rf;
pub mod sensors;

// Re-export domain crates under a unified namespace
pub use mediamaster_audio as audio_domain;
pub use mediamaster_rf as rf_domain;
pub use mediamaster_sensors as sensors_domain;

// Re-export the core traits the applications implement
pub use mediamaster_core::traits::{AsyncSensor, AudioSource, SignalDetector};
pub use mediamaster_core::types::{RoomState, SensorReading, Signal};

// Re-export the bridge entry points
pub use audio::AudioBridge;
pub use events::EventHandlers;
pub use rf::RfBridge;
pub use sensors::SensorBridge;
