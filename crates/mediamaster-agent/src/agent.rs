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
use mediamaster_rag::{
    AcousticProfile, CodecQuery, MediamasterRagFacade, RagContextEnvelope, SceneQuery,
    SensorVector, TemporalMetadata, TriggerPattern,
};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

/// Configuration for adaptive ducking sensitivity derived from scene memory and playbooks.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveDuckingConfig {
    pub target_gain: f32,
    pub hold_duration_ms: u64,
}

impl Default for AdaptiveDuckingConfig {
    fn default() -> Self {
        Self {
            target_gain: 0.0,
            hold_duration_ms: 600,
        }
    }
}

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
    /// Optional RAG facade providing background context without blocking audio mixing.
    rag: Option<Arc<RwLock<MediamasterRagFacade>>>,
    /// Dynamically adjusted ducking sensitivity updated by scene memory.
    adaptive_ducking: Arc<Mutex<AdaptiveDuckingConfig>>,
}

impl Agent {
    /// Build a new agent.
    ///
    /// The agent does not start the loop on construction: call [`Agent::run`].
    pub fn new(bus: Arc<EventBus>, audio: Arc<AudioBridge>) -> Self {
        Self {
            bus,
            audio,
            rag: None,
            adaptive_ducking: Arc::new(Mutex::new(AdaptiveDuckingConfig::default())),
        }
    }

    /// Attach a RAG facade as an asynchronous background context provider.
    pub fn with_rag(mut self, rag: Arc<RwLock<MediamasterRagFacade>>) -> Self {
        self.rag = Some(rag);
        self
    }

    /// Access the attached RAG facade, if any.
    pub fn rag(&self) -> Option<&Arc<RwLock<MediamasterRagFacade>>> {
        self.rag.as_ref()
    }

    /// Access the adaptive ducking configuration.
    pub fn adaptive_ducking_config(&self) -> AdaptiveDuckingConfig {
        self.adaptive_ducking
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Asynchronously generates a unified RAG context envelope without blocking
    /// real-time audio threads or the agent event loop.
    pub async fn get_context_envelope(
        &self,
        timestamp_ms: u64,
        codec_query: Option<&CodecQuery>,
        scene_query: Option<&SceneQuery>,
        trigger: Option<(&TriggerPattern, Option<&AcousticProfile>)>,
    ) -> Option<RagContextEnvelope> {
        if let Some(rag_lock) = &self.rag {
            let facade = rag_lock.read().await;
            Some(facade.build_context_envelope(timestamp_ms, codec_query, scene_query, trigger))
        } else {
            None
        }
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
            HardwareEvent::ZoneChanged { .. }
            | HardwareEvent::CctvMotionDetected { .. }
            | HardwareEvent::AudioUnderrun { .. }
            | HardwareEvent::AudioOverrun { .. } => {
            | HardwareEvent::PosterGenerated { .. }
            | HardwareEvent::EvictionPlanned { .. } => {
            }
            HardwareEvent::EnvironmentalDrift {
                motion_density,
                mmwave_distance_m,
                ambient_lux,
                lux_rate_of_change,
                rf_burst_frequency,
            } => {
                self.on_environmental_drift(
                    *motion_density,
                    *mmwave_distance_m,
                    *ambient_lux,
                    *lux_rate_of_change,
                    *rf_burst_frequency,
                );
            }
            HardwareEvent::SlotRevoked { .. }
            | HardwareEvent::StarvationDetected { .. }
            | HardwareEvent::DecodeHeartbeat { .. } => {
                // Codec telemetry events are consumed by TelemetryIngest in mediamaster-rag.
            }
        }
        Ok(())
    }

    fn on_environmental_drift(
        &self,
        motion_density: f32,
        mmwave_distance_m: f32,
        ambient_lux: f32,
        lux_rate_of_change: f32,
        rf_burst_frequency: f32,
    ) {
        let sensor_vec = SensorVector::new(
            motion_density,
            mmwave_distance_m,
            ambient_lux,
            lux_rate_of_change,
            rf_burst_frequency,
        );

        // If RAG is configured, perform non-blocking query to match scene memory
        if let Some(rag_lock) = &self.rag {
            let rag_clone = Arc::clone(rag_lock);
            let audio_clone = Arc::clone(&self.audio);
            let adaptive_ducking_clone = Arc::clone(&self.adaptive_ducking);

            tokio::spawn(async move {
                let query = SceneQuery {
                    current_sensors: sensor_vec,
                    current_temporal: TemporalMetadata::new(1, 12, false, "Standard"),
                };

                let playbook = {
                    let facade = rag_clone.read().await;
                    facade.query_scene(&query).ok()
                };

                if let Some(playbook) = playbook {
                    // 1. Smooth crossfade to the recommended master level
                    let crossfade_ms = playbook.audio_state.crossfade_duration_ms.max(100);
                    // Convert master volume dB to linear gain: 10^(dB / 20)
                    let target_gain = (10.0f32.powf(playbook.audio_state.master_volume_db / 20.0))
                        .clamp(0.0, 1.0);

                    audio_clone.set_ducking_profile(DuckingProfile {
                        curve: DuckingCurve::Linear,
                        duration_ms: crossfade_ms,
                        target: target_gain,
                    });

                    // 2. Adjust adaptive ducking sensitivity
                    if let Ok(mut config) = adaptive_ducking_clone.lock() {
                        config.target_gain = playbook.audio_state.ducking_target_gain;
                        config.hold_duration_ms = playbook.audio_state.ducking_hold_ms;
                    }
                }
            });
        }
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
        // Read adaptive ducking configuration
        let config = self.adaptive_ducking_config();

        // Live paging / signal presence: duck the output and audit it.
        self.audio.set_ducking_profile(DuckingProfile {
            curve: DuckingCurve::Exponential,
            duration_ms: config.hold_duration_ms,
            target: config.target_gain,
        });
        let _ = self.bus.publish(HardwareEvent::AudioDuckRequest {
            source: burst.id.clone(),
            ducking: config.target_gain,
            until: config.hold_duration_ms,
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

    #[tokio::test]
    async fn background_rag_context_provider_envelope() {
        let bus = new_bus();
        let audio = new_bridge();
        let facade = Arc::new(RwLock::new(MediamasterRagFacade::new(5, 5)));
        let agent = Agent::new(bus, audio).with_rag(facade);

        let envelope = agent
            .get_context_envelope(
                1700000000000,
                None,
                None,
                Some((&TriggerPattern::EmergencyOverride, None)),
            )
            .await
            .expect("expected envelope from attached RAG");

        assert_eq!(envelope.domain, "mediamaster-rs");
        assert!(envelope.paging_context.is_some());
        assert!(envelope.prompt_summary.contains("PAGING"));
    }

    #[tokio::test]
    async fn environmental_drift_triggers_crossfade_and_adaptive_ducking() {
        let bus = new_bus();
        let audio = new_bridge();
        let mut rag_facade = MediamasterRagFacade::new(5, 5);

        // Pre-populate scene record with unique low ambient atmosphere
        rag_facade
            .scenes
            .record_scene(mediamaster_rag::SceneRecord {
                sensor_vector: SensorVector::new(0.05, 8.0, 20.0, -0.5, 0.0),
                temporal: TemporalMetadata::new(1, 12, false, "Standard"),
                playbook: mediamaster_rag::AtmospherePlaybook {
                    scene_label: "LowAmbientStore".into(),
                    audio_state: mediamaster_rag::OptimalAudioState {
                        master_volume_db: -6.0,
                        ducking_target_gain: 0.25,
                        ducking_hold_ms: 1500,
                        acoustic_preset: "ChillBackground".into(),
                        crossfade_duration_ms: 250,
                    },
                    match_confidence: 1.0,
                    description: "Low ambient evening music".into(),
                },
            });

        let rag = Arc::new(RwLock::new(rag_facade));
        let agent = Agent::new(bus.clone(), audio.clone()).with_rag(rag);

        // Dispatch EnvironmentalDrift event
        agent.dispatch(&HardwareEvent::EnvironmentalDrift {
            motion_density: 0.06,
            mmwave_distance_m: 7.9,
            ambient_lux: 21.0,
            lux_rate_of_change: -0.4,
            rf_burst_frequency: 0.0,
        });

        // Yield to background tokio task
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify adaptive ducking config was updated
        let config = agent.adaptive_ducking_config();
        assert_eq!(config.target_gain, 0.25);
        assert_eq!(config.hold_duration_ms, 1500);

        // Verify audio crossfade ducking profile was applied (target gain 10^(-6/20) ~ 0.501)
        assert!(audio.mixer().lock().unwrap().ducking() < 0.9);

        // Verify next paging burst uses the updated adaptive hold time and target gain
        let mut rx = bus.subscribe();
        agent.dispatch(&HardwareEvent::RadioBurstDetected {
            burst: mediamaster_core::RadioBurst {
                id: "pager-1".into(),
                frequency_hz: 433_920_000,
                power_db: 0.8,
                payload: vec![0x10],
                detected_by: "OOK".into(),
            },
        });

        let event = rx.try_recv().unwrap();
        if let HardwareEvent::AudioDuckRequest { ducking, until, .. } = event {
            assert_eq!(ducking, 0.25);
            assert_eq!(until, 1500);
        } else {
            panic!("expected AudioDuckRequest");
        }
    }
}
