//! `EventBus` → domain-bridge routing.
//!
//! [`EventHandlers`] subscribes to an [`EventBus`] and dispatches each
//! [`HardwareEvent`] to the right domain: audio duck requests drive the
//! [`AudioBridge`]'s mixer, while sensor/fusion and RF results are recorded
//! for the application to poll.

use std::sync::{Arc, Mutex};

use tokio::sync::broadcast::error::RecvError;

use mediamaster_audio::ducking::AutomationCommand;
use mediamaster_bus::EventBus;
use mediamaster_core::events::HardwareEvent;
use mediamaster_core::types::{RadioBurst, RoomState};

use crate::audio::AudioBridge;

/// Recovers the inner value of a `Mutex` even if a previous holder poisoned
/// the lock (the value is still usable; the panic would only hide the real
/// earlier failure).
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Routes [`EventBus`] events to the domain bridges.
///
/// The application builds one `EventHandlers` over its own [`AudioBridge`]
/// handle, then either pushes events one at a time with [`Self::handle_event`] or
/// spawns [`Self::run`] to subscribe to the bus and dispatch automatically.
///
/// Routing:
/// - [`AudioDuckRequest`](HardwareEvent::AudioDuckRequest) →
///   [`AudioBridge::push_automation`], building an [`mediamaster_audio::ducking::AutomationCommand`]
///   and easing the mixer gain per-sample via the lock-free IIR smoother
///   (the `source` name is informational; the mixer ducks globally).
/// - [`SensorContextChange`](HardwareEvent::SensorContextChange) → recorded as
///   the latest [`RoomState`] (see [`Self::last_room_state`]).
/// - [`RadioBurstDetected`](HardwareEvent::RadioBurstDetected) → recorded as
///   the latest [`RadioBurst`] (see [`Self::last_burst`]).
/// - [`ZoneChanged`](HardwareEvent::ZoneChanged) /
///   [`CctvMotionDetected`](HardwareEvent::CctvMotionDetected) → no-op (this
///   workspace has no zone or CCTV bridge; the apps own those surfaces).
pub struct EventHandlers {
    audio: Arc<AudioBridge>,
    last_room_state: Mutex<Option<RoomState>>,
    last_burst: Mutex<Option<RadioBurst>>,
}

impl EventHandlers {
    /// Build handlers over the application's audio bridge.
    pub fn new(audio: Arc<AudioBridge>) -> Self {
        Self {
            audio,
            last_room_state: Mutex::new(None),
            last_burst: Mutex::new(None),
        }
    }

    /// Dispatch a single event (synchronous; used by `run` and by tests or
    /// custom app loops).
    pub fn handle_event(&self, event: &HardwareEvent) {
        match event {
            HardwareEvent::AudioDuckRequest { ducking, until, .. } => {
                // Lock-free path: build an AutomationCommand and push it to
                // the mixer's per-sample IIR smoother. No mutex on the render
                // thread, no heap, no `.await`.
                let coeff = AutomationCommand::coeff_for(*until as u32, 48_000);
                self.audio.push_automation(AutomationCommand::DuckStart {
                    target_gain: *ducking,
                    coeff,
                });
            }
            HardwareEvent::SensorContextChange { state } => {
                *lock(&self.last_room_state) = Some(*state);
            }
            HardwareEvent::RadioBurstDetected { burst } => {
                *lock(&self.last_burst) = Some(burst.clone());
            }
            HardwareEvent::ZoneChanged { .. }
            | HardwareEvent::CctvMotionDetected { .. }
            | HardwareEvent::EnvironmentalDrift { .. } => {}
        }
    }

    /// Latest fused room state recorded from a
    /// [`SensorContextChange`](HardwareEvent::SensorContextChange).
    pub fn last_room_state(&self) -> Option<RoomState> {
        *lock(&self.last_room_state)
    }

    /// Latest RF burst recorded from a
    /// [`RadioBurstDetected`](HardwareEvent::RadioBurstDetected).
    pub fn last_burst(&self) -> Option<RadioBurst> {
        lock(&self.last_burst).clone()
    }

    /// Subscribe to `bus` and dispatch every event until the bus is dropped.
    ///
    /// Spawns no task of its own — `run` is a future you `await` (directly or
    /// via `tokio::spawn`). It ends when all [`EventBus`] senders are dropped,
    /// so the application shuts the loop down by dropping its `EventBus`.
    pub async fn run(self: Arc<Self>, bus: Arc<EventBus>) {
        let mut rx = bus.subscribe();
        // Hold only the receiver: dropping our `EventBus` handle lets the app's
        // drop close the channel and end this loop.
        drop(bus);
        loop {
            match rx.recv().await {
                Ok(event) => self.handle_event(&event),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duck_request_eases_mixer_gain() {
        use mediamaster_core::traits::AudioSource;

        struct ConstSrc;
        impl AudioSource for ConstSrc {
            fn read_pcm(&self, buffer: &mut [f32]) -> usize {
                for s in buffer.iter_mut() {
                    *s = 1.0;
                }
                buffer.len()
            }
            fn label(&self) -> &str {
                "const"
            }
        }

        let audio = Arc::new(AudioBridge::new());
        audio.mixer().lock().unwrap().add_source(Box::new(ConstSrc));
        let handlers = EventHandlers::new(audio.clone());
        assert_eq!(audio.mixer().lock().unwrap().ducking(), 1.0);

        handlers.handle_event(&HardwareEvent::AudioDuckRequest {
            source: "announcer".into(),
            ducking: 0.0,
            until: 5_000,
        });
        let _ = audio.mixer().lock().unwrap().mix();
        assert!(
            audio.mixer().lock().unwrap().iir_gain() < 1.0,
            "duck request must ease the mixer gain"
        );
    }

    #[test]
    fn sensor_context_change_records_room_state() {
        let handlers = EventHandlers::new(Arc::new(AudioBridge::new()));
        assert_eq!(handlers.last_room_state(), None);
        handlers.handle_event(&HardwareEvent::SensorContextChange {
            state: RoomState::Occupied,
        });
        assert_eq!(handlers.last_room_state(), Some(RoomState::Occupied));
        handlers.handle_event(&HardwareEvent::SensorContextChange {
            state: RoomState::Empty,
        });
        assert_eq!(handlers.last_room_state(), Some(RoomState::Empty));
    }

    #[test]
    fn radio_burst_records_latest_burst() {
        let handlers = EventHandlers::new(Arc::new(AudioBridge::new()));
        let burst = RadioBurst {
            id: "bin-3".into(),
            frequency_hz: 433_920_000,
            power_db: 0.9,
            payload: vec![0x01, 0x02],
            detected_by: "OOK@433920000Hz".into(),
        };
        handlers.handle_event(&HardwareEvent::RadioBurstDetected {
            burst: burst.clone(),
        });
        assert_eq!(handlers.last_burst(), Some(burst));
    }

    #[test]
    fn zone_and_cctv_events_are_ignored() {
        let handlers = EventHandlers::new(Arc::new(AudioBridge::new()));
        handlers.handle_event(&HardwareEvent::ZoneChanged {
            zone_ids: vec!["lobby".into()],
            volume_db: -6.0,
            muted: false,
        });
        handlers.handle_event(&HardwareEvent::CctvMotionDetected {
            camera: "cam-1".into(),
            bounding_box: mediamaster_core::types::BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        });
        assert_eq!(handlers.last_room_state(), None);
        assert_eq!(handlers.last_burst(), None);
    }

    #[tokio::test]
    async fn run_dispatches_published_events_until_bus_drops() {
        let handlers = Arc::new(EventHandlers::new(Arc::new(AudioBridge::new())));
        let bus = Arc::new(EventBus::new(16));

        let run_handlers = handlers.clone();
        let run_bus = bus.clone();
        let task = tokio::spawn(async move { run_handlers.run(run_bus).await });

        // Let the spawned loop reach its `subscribe()` before publishing.
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let _ = bus.publish(HardwareEvent::SensorContextChange {
            state: RoomState::PersonPresent,
        });
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        assert_eq!(handlers.last_room_state(), Some(RoomState::PersonPresent));

        // Dropping the app's last `EventBus` handle closes the channel and
        // ends the run loop.
        drop(bus);
        task.await.expect("run loop exits when the bus closes");
    }
}
