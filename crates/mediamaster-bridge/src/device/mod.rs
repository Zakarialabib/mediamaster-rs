//! Device enumerators and event handlers.
//!
//! This module provides:
//! - **Device enumerators**: enumerate available audio interfaces, sensors,
//!   and SDR devices (internal + external)
//! - **Event handlers**: react to `EventBus` events (`AudioDuckRequest`,
//!   `SensorContextChange`, `RadioBurstDetected`)

/// Event handlers that subscribe to the `EventBus` and route events to the
/// appropriate domain bridges.
pub struct EventHandlers;

impl Default for EventHandlers {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHandlers {
    /// Create a new event handlers instance.
    pub fn new() -> Self {
        Self
    }

    /// Subscribe to the event bus and handle events.
    pub fn subscribe_to_bus(&self) {
        // Placeholder: applications subscribe to EventBus and handle events.
        // The handlers route events to the appropriate domain bridges.
    }
}
