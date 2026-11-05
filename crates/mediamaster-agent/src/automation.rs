//! Automation rules and driver mapping RoomState transitions to concrete actions.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mediamaster_audio::ducking::{DuckingCurve, DuckingProfile};
use mediamaster_bridge::AudioBridge;
use mediamaster_bus::EventBus;
use mediamaster_core::{HardwareEvent, RoomState};

/// Target for an automation action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionTarget {
    /// Ambient audio zone or global mixer master volume.
    AmbientZone {
        /// Target linear gain (e.g. 1.0 for unity gain).
        gain: f32,
        /// Transition ramp duration in milliseconds.
        #[serde(default = "default_duration_ms")]
        duration_ms: u64,
    },
    /// Paging queue control.
    PagingQueue {
        /// Whether the queue should be paused.
        paused: bool,
    },
    /// Emergency ducking applied to non-priority zones / global mixer.
    EmergencyDucking {
        /// Target gain during emergency (usually 0.0 for full mute).
        #[serde(default)]
        target_gain: f32,
        /// Duration of the emergency ducking window in milliseconds.
        #[serde(default = "default_emergency_duration_ms")]
        duration_ms: u64,
        /// Ducking curve to apply.
        #[serde(default = "default_ducking_curve")]
        curve: String,
    },
}

fn default_duration_ms() -> u64 {
    200
}

fn default_emergency_duration_ms() -> u64 {
    500
}

fn default_ducking_curve() -> String {
    "exponential".to_string()
}

/// A declarative automation rule matching a room state to an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationRule {
    /// State that triggers this rule.
    pub state: String,
    /// Action to execute when state is active or entered.
    pub action: ActionTarget,
}

/// A collection of automation rules, serde-loadable from TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutomationRuleSet {
    #[serde(default)]
    pub rules: Vec<AutomationRule>,
}

impl AutomationRuleSet {
    /// Load an automation rule set from a TOML string.
    pub fn from_toml(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// Default rule set matching the specification:
    /// - Occupied -> ambient zone gain up
    /// - Vacancy (Empty) -> pause paging queue
    /// - EmergencyOverride -> attach DuckingProfile(emergency) to non-priority zones
    pub fn default_preset() -> Self {
        Self {
            rules: vec![
                AutomationRule {
                    state: "Occupied".to_string(),
                    action: ActionTarget::AmbientZone {
                        gain: 1.0,
                        duration_ms: 200,
                    },
                },
                AutomationRule {
                    state: "Vacancy".to_string(),
                    action: ActionTarget::PagingQueue { paused: true },
                },
                AutomationRule {
                    state: "EmergencyOverride".to_string(),
                    action: ActionTarget::EmergencyDucking {
                        target_gain: 0.0,
                        duration_ms: 600,
                        curve: "exponential".to_string(),
                    },
                },
            ],
        }
    }
}

/// Subscribes to `RoomState` transitions from `FusionEngine` (via the shared `EventBus`)
/// and maps states to actions declaratively based on an `AutomationRuleSet`.
pub struct FusionOutputDriver {
    bus: Arc<EventBus>,
    audio: Arc<AudioBridge>,
    rules: AutomationRuleSet,
    paging_paused: Arc<AtomicBool>,
}

impl FusionOutputDriver {
    pub fn new(bus: Arc<EventBus>, audio: Arc<AudioBridge>, rules: AutomationRuleSet) -> Self {
        Self {
            bus,
            audio,
            rules,
            paging_paused: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if the paging queue is currently paused.
    pub fn is_paging_paused(&self) -> bool {
        self.paging_paused.load(Ordering::SeqCst)
    }

    /// Access the current rule set.
    pub fn rules(&self) -> &AutomationRuleSet {
        &self.rules
    }

    /// Process a room state transition according to the configured rule set.
    pub fn on_state_transition(&self, state: RoomState) {
        let state_name = match state {
            RoomState::Empty => "Vacancy",
            RoomState::Occupied => "Occupied",
            RoomState::LowLight => "LowLight",
            RoomState::MotionDetected => "MotionDetected",
            RoomState::PersonPresent => "PersonPresent",
            RoomState::EmergencyOverride => "EmergencyOverride",
            RoomState::Unavailable => "Unavailable",
        };

        for rule in &self.rules.rules {
            if rule.state == state_name || (rule.state == "Empty" && state_name == "Vacancy") {
                self.execute_action(&rule.action);
            }
        }
    }

    fn execute_action(&self, action: &ActionTarget) {
        match action {
            ActionTarget::AmbientZone { gain, duration_ms } => {
                self.audio.set_ducking_profile(DuckingProfile {
                    curve: DuckingCurve::Linear,
                    duration_ms: *duration_ms,
                    target: *gain,
                });
                let _ = self.bus.publish(HardwareEvent::AudioDuckRequest {
                    source: "fusion_driver_ambient".to_string(),
                    ducking: *gain,
                    until: *duration_ms,
                });
            }
            ActionTarget::PagingQueue { paused } => {
                self.paging_paused.store(*paused, Ordering::SeqCst);
            }
            ActionTarget::EmergencyDucking {
                target_gain,
                duration_ms,
                curve,
            } => {
                let ducking_curve = match curve.to_lowercase().as_str() {
                    "linear" => DuckingCurve::Linear,
                    "instant" => DuckingCurve::Instant,
                    _ => DuckingCurve::Exponential,
                };
                self.audio.set_ducking_profile(DuckingProfile {
                    curve: ducking_curve,
                    duration_ms: *duration_ms,
                    target: *target_gain,
                });
                let _ = self.bus.publish(HardwareEvent::AudioDuckRequest {
                    source: "fusion_driver_emergency".to_string(),
                    ducking: *target_gain,
                    until: *duration_ms,
                });
            }
        }
    }

    /// Asynchronously runs the subscription loop, reacting to `SensorContextChange` events.
    pub async fn run(self: Arc<Self>) {
        let mut rx = self.bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let HardwareEvent::SensorContextChange { state } = event {
                        self.on_state_transition(state);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rules_from_toml() {
        let toml_data = r#"
[[rules]]
state = "Occupied"
[rules.action]
type = "ambient_zone"
gain = 0.95
duration_ms = 150

[[rules]]
state = "Vacancy"
[rules.action]
type = "paging_queue"
paused = true

[[rules]]
state = "EmergencyOverride"
[rules.action]
type = "emergency_ducking"
target_gain = 0.0
duration_ms = 500
curve = "instant"
"#;
        let set = AutomationRuleSet::from_toml(toml_data).unwrap();
        assert_eq!(set.rules.len(), 3);
        assert_eq!(set.rules[0].state, "Occupied");
        assert_eq!(
            set.rules[0].action,
            ActionTarget::AmbientZone {
                gain: 0.95,
                duration_ms: 150
            }
        );
        assert_eq!(
            set.rules[1].action,
            ActionTarget::PagingQueue { paused: true }
        );
        assert_eq!(
            set.rules[2].action,
            ActionTarget::EmergencyDucking {
                target_gain: 0.0,
                duration_ms: 500,
                curve: "instant".to_string(),
            }
        );
    }

    #[test]
    fn driver_dispatches_actions() {
        let bus = Arc::new(EventBus::new(16));
        let audio = Arc::new(AudioBridge::new());
        let driver = FusionOutputDriver::new(
            bus.clone(),
            audio.clone(),
            AutomationRuleSet::default_preset(),
        );

        // Test Vacancy -> pauses paging
        assert!(!driver.is_paging_paused());
        driver.on_state_transition(RoomState::Empty);
        assert!(driver.is_paging_paused());

        // Test Occupied -> ambient zone gain up
        driver.on_state_transition(RoomState::Occupied);
        assert_eq!(audio.mixer().lock().unwrap().ducking(), 1.0);

        // Test EmergencyOverride -> ducks mixer to 0.0
        driver.on_state_transition(RoomState::EmergencyOverride);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(audio.mixer().lock().unwrap().ducking() < 1.0);
    }
}
