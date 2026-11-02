//! Live Paging & Sensor Ducking Loop.
//!
//! Subscribes to the shared `EventBus` and dispatches each incoming event into a
//! **ducking action**: on `RoomState::EmergencyOverride` or a live
//! `RadioBurstDetected` (paging stream), the audio mixer is eased to `FULL_MUTE`
//! for `duck_duration_ms` and an audit `AudioDuckRequest` is published.
//!
//! This is the 0.3.0 coordination contract uniting `mediamaster-audio`
//! (voice ducking), `mediamaster-sensors` (context fusion) and
//! `mediamaster-rf` (signal presence) — each of them publishes into the same
//! `EventBus`, so the agent only ever has to listen to one stream.

use mediamaster_audio::ducking::{DuckingCurve, DuckingProfile};
use mediamaster_bridge::AudioBridge;
use mediamaster_bus::EventBus;
use mediamaster_core::{HardwareEvent, RoomState};
use std::sync::Arc;

/// Run-time error for the `mediamaster_agent` coordination loop.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    /// The event bus closed while the loop was waiting.
    #[error("event bus closed")]
    BusClosed,
    /// A `publish` dropped an audit event because no subscriber was attached.
    #[error("bus full: {0}")]
    BusFull(String),
}

/// Live Paging & Sensor Ducking Loop.
///
/// Unites `mediamaster-audio` (voice ducking), `mediamaster-sensors` (context
/// fusion) and `mediamaster-rf` (signal presence) under one coordination loop:
/// the host drives each engine and wires its output into the same `EventBus`,
/// and this agent only subscribes — dispatching any `RadioBurstDetected` or
/// `SensorContextChange` event into an audio-ducking action.
///
/// # Usage
///
/// ```no_run
/// use std::sync::Arc;
/// use mediamaster_bus::EventBus;
/// use mediamaster_agent::Agent;
/// use mediamaster_bridge::AudioBridge;
/// fn main() {
///     let rt = tokio::runtime::Runtime::new().unwrap();
///     rt.block_on(async {
///         let bus = Arc::new(mediamaster_bus::EventBus::new(16));
///         let audio = Arc::new(mediamaster_bridge::AudioBridge::new());
///         let agent = Agent::new(bus.clone(), audio);
///         agent.run().await.expect("agent loop");
///     });
/// }
/// ```
pub struct Agent {
    /// Shared event bus (all three engines feed it).
    bus: Arc<EventBus>,
    /// The audio domain bridge — the agent drives its `Mixer` ducking.
    audio: Arc<AudioBridge>,
}

impl Agent {
    /// Build a new agent.
    ///
    /// The agent does not start the loop on construction: call [`Agent::run`].
    pub fn new(bus: Arc<EventBus>, audio: Arc<AudioBridge>) -> Self {
        Self { bus, audio }
    }

    /// Restart the coordination loop. Consumes `self` and drives the loop until
    /// the event bus is closed.
    pub async fn run(self) -> Result<(), AgentError> {
        let mut rx = self.bus.subscribe();
        loop {
            match rx.recv().await {
                Err(_) => {
                    // The sender was dropped (shutdown); the loop is done.
                    return Ok(());
                }
                Ok(ref event) => {
                    self.handle_event(event)?;
                }
            }
        }
    }

    /// Synchronously dispatch a single event (no runtime required). Useful for
    /// tests and for app-level administration paths.
    pub fn dispatch(&self, event: &HardwareEvent) {
        let _ = self.handle_event(event);
    }

    fn handle_event(&self, event: &HardwareEvent) -> Result<(), AgentError> {
        match event {
            HardwareEvent::SensorContextChange { state } => {
                self.on_room_state(*state);
            }
            HardwareEvent::RadioBurstDetected { burst } => {
                // A live paging / protocol burst: mark the stream active and duck.
                self.on_signal(burst);
            }
            HardwareEvent::AudioDuckRequest { ducking, until, .. } => {
                // Administrative duck request: honour it on the audio domain.
                self.audio.set_ducking_profile(DuckingProfile {
                    curve: DuckingCurve::Exponential,
                    duration_ms: *until,
                    target: *ducking,
                });
            }
            HardwareEvent::ZoneChanged { .. } | HardwareEvent::CctvMotionDetected { .. } => {
                // Zone/Cctv events are recording-only for the agent.
            }
        }
        Ok(())
    }

    fn on_room_state(&self, state: RoomState) {
        match state {
            RoomState::EmergencyOverride => {
                self.start_ducking(DuckingProfile {
                    curve: DuckingCurve::Exponential,
                    duration_ms: 300,
                    target: 0.0,
                });
            }
            _ => {
                // Zone state change: record only (the mixer is left untouched).
            }
        }
    }

    fn on_signal(&self, burst: &mediamaster_core::RadioBurst) {
        // Live paging / signal presence: duck the output and audit it.
        self.audio.set_ducking_profile(DuckingProfile {
            curve: DuckingCurve::Exponential,
            duration_ms: 600,
            target: 0.0,
        });
        let _ = self.bus.publish(HardwareEvent::AudioDuckRequest {
            source: burst.id.clone(),
            ducking: 0.0,
            until: 600,
        });
    }

    fn start_ducking(&self, profile: DuckingProfile) {
        // The audio bridge owns the mixer behind a Mutex; apply the profile
        // atomically from the async plane.
        self.audio.set_ducking_profile(profile.clone());
        // Audit the transition.
        let _ = self.bus.publish(HardwareEvent::AudioDuckRequest {
            source: "agent".to_string(),
            ducking: 0.0,
            until: profile.duration_ms,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::RoomState;

    fn new_bridge() -> Arc<mediamaster_bridge::AudioBridge> {
        Arc::new(mediamaster_bridge::AudioBridge::new())
    }

    fn new_bus() -> Arc<mediamaster_bus::EventBus> {
        Arc::new(mediamaster_bus::EventBus::new(16))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn run_subscribes_and_ducks_on_emergency_override() {
        let bus = new_bus();
        let audio = new_bridge();
        let agent = Agent::new(Arc::clone(&bus), audio.clone());
        tokio::spawn(async move { agent.run().await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = bus.publish(HardwareEvent::SensorContextChange {
            state: RoomState::EmergencyOverride,
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(audio.mixer().lock().unwrap().ducking() < 1.0);
    }

    #[test]
    fn dispatch_emergency_override_ducks_mixer() {
        let audio = new_bridge();
        Agent::new(new_bus(), audio.clone()).dispatch(&HardwareEvent::SensorContextChange {
            state: RoomState::EmergencyOverride,
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(audio.mixer().lock().unwrap().ducking() < 1.0);
    }

    #[test]
    fn dispatch_non_emergency_does_not_duck() {
        let audio = new_bridge();
        Agent::new(new_bus(), audio.clone()).dispatch(&HardwareEvent::SensorContextChange {
            state: RoomState::Occupied,
        });
        assert!((audio.mixer().lock().unwrap().ducking() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dispatch_radio_burst_ducks_and_audits() {
        let bus = new_bus();
        let audio = new_bridge();
        let mut rx = bus.subscribe();
        Agent::new(Arc::clone(&bus), audio.clone()).dispatch(&HardwareEvent::RadioBurstDetected {
            burst: mediamaster_core::RadioBurst {
                id: "p1".to_string(),
                frequency_hz: 433_920_000,
                power_db: 0.9,
                payload: vec![0x01, 0x02],
                detected_by: "OOK@433MHz".to_string(),
            },
        });
        let received = rx.try_recv().unwrap();
        match received {
            HardwareEvent::AudioDuckRequest { source, .. } => assert_eq!(source, "p1"),
            _ => panic!("expected AudioDuckRequest audit, got {received:?}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(audio.mixer().lock().unwrap().ducking() < 1.0);
    }
}
