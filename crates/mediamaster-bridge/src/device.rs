//! Device registry, event bus, and enumerator wiring.
//!
//! This module provides the unified application-facing API for the
//! `mediamaster-rs` bridges:
//!
//! - **`DeviceRegistry`** — query all available bridges (audio, sensors, rf)
//!   with their capabilities, so the UI can build device selection dynamically.
//! - **`AppEvent` / `AppCommand`** — a unified event bus for the apps
//!   (e.g. `AppCommand::TriggerPage` to page a room, `AppEvent::SensorUpdate`
//!   for sensor/RF updates), so the desktop + phone share one surface.
//! - **`DeviceInfo`** — serializable device metadata for the app's UI state.

use serde::{Deserialize, Serialize};

/// A device/bridge as seen by the application layer.
///
/// This is the data the UI consumes to build device selection menus,
/// settings panels, and a live device dashboard. It is `Serialize`/
/// `Deserialize` so the app can snapshot device state and sync across
/// desktop + phone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique identifier for the device type (e.g. "mediamaster-bridge/audio").
    pub id: String,
    /// Human-readable display name.
    pub label: String,
    /// Capability flags this bridge supports.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Whether this device is currently available.
    pub available: bool,
    /// Platform-specific device handle (optional; not serialized for security).
    #[serde(skip_serializing, skip_deserializing)]
    pub handle: Option<String>,
}

impl DeviceInfo {
    /// Build a `DeviceInfo` from a bridge snippet and capability set.
    pub fn new(id: &str, label: &str, available: bool, capabilities: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            available,
            handle: None,
        }
    }
}

/// The application-visible event types. The app can subscribe to these
/// asynchronously (e.g. via a `tokio::sync::mpsc` channel) to handle sensor
/// updates, RF bursts, and audio ducking requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppEvent {
    /// A room-level event (e.g. zone changed, scene triggered).
    RoomChanged { room_id: String },
    /// A sensor or RF detector reported a reading.
    SensorUpdate {
        sensor_id: String,
        reading: f32,
        unit: String,
    },
    /// An RF detector reported a burst (from `mediamaster_rf`).
    RfBurstDetected {
        frequency_hz: u32,
        amplitude: f32,
        timestamp: u64,
    },
    /// A ducking request from the app to the audio bridge.
    AudioDuckRequest { until_ms: u64, target: f32 },
}

/// Application commands the bridge can dispatch to the domain crates.
///
/// The app sends these to the bridge over its event bus; the bridge translates
/// them into domain-crate calls (e.g. `AppCommand::TriggerPage` → `AudioBridge`
/// paging, `SensorContextChange` → fusion engine).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppCommand {
    /// Trigger a page (prompt) — tells the audio bridge to start a paging
    /// stream to the room.
    TriggerPage { room_id: String },
    /// Exit a page.
    EndPage { room_id: String },
    /// Set the active room/sensor context (affects ducking targets).
    SetRoomContext { room_id: String },
    /// Request a sensor reading (blocking in the app's context — the bridge
    /// lifts this to async in the integration layer).
    RequestSensor { sensor_id: String },
}

/// A trait for querying the available bridges and their capabilities.
///
/// The application builds a `DeviceRegistry` and queries it at startup and
/// on-device-connect events to build the UI. Each bridge is a separate
/// struct implementing `DeviceRegistry`, and the app can combine them.
pub trait DeviceRegistry: Send + Sync {
    /// List all devices of this registry's type (e.g. audio, sensors, rf).
    fn enumerate_devices(&self) -> Vec<DeviceInfo>;

    /// Check whether a device of a given id is available.
    fn is_available(&self, id: &str) -> bool {
        self.enumerate_devices()
            .iter()
            .any(|d| d.id == id && d.available)
    }
}

/// Concrete `DeviceRegistry` that composes multiple bridge registries.
///
/// The app builds one `DeviceRegistry` from all its bridges:
///
/// ```ignore
/// use mediamaster_bridge::device::*;
/// use mediamaster_bridge::AudioBridge;
///
/// let audio = AudioBridge::new();
/// let sensor_registry = ...;
/// let mut registry = DeviceRegistryImpl::new();
/// registry.add("audio", Box::new(audio));
/// // ...
/// let devices = registry.enumerate_devices();
/// ```
pub struct DeviceRegistryImpl {
    devices: Vec<Box<dyn DeviceRegistry>>,
}

impl DeviceRegistryImpl {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    /// Add a bridge to the registry.
    pub fn add<T: DeviceRegistry + 'static>(&mut self, registry: T) {
        self.devices.push(Box::new(registry));
    }
}

impl DeviceRegistry for DeviceRegistryImpl {
    fn enumerate_devices(&self) -> Vec<DeviceInfo> {
        let mut out = Vec::new();
        for reg in &self.devices {
            out.extend(reg.enumerate_devices());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_info_serializes() {
        let info = DeviceInfo::new("audio", "built-in speakers", true, &["mix", "duck"]);
        let serialized = serde_json::to_string(&info).unwrap();
        assert!(serialized.contains("audio"));
        assert!(serialized.contains("built-in speakers"));
    }

    #[test]
    fn device_registry_impl_aggregates() {
        let mut reg = DeviceRegistryImpl::new();
        reg.add(DummyRegistry);
        reg.add(DummyRegistry);
        let devices = reg.enumerate_devices();
        assert_eq!(devices.len(), 2);
    }
}

struct DummyRegistry;
impl DeviceRegistry for DummyRegistry {
    fn enumerate_devices(&self) -> Vec<DeviceInfo> {
        vec![DeviceInfo::new("dummy", "dummy device", true, &["test"])]
    }
}
