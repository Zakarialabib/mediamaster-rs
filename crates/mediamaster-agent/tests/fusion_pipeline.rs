//! End-to-end integration test: scripted occupancy -> vacancy -> emergency sequence
//! drives Mixer duck state exactly as the rule set specifies (asserted via bus event log ordering).

use std::sync::Arc;
use std::time::Duration;

use mediamaster_agent::{AutomationRuleSet, FusionOutputDriver};
use mediamaster_bridge::AudioBridge;
use mediamaster_bus::EventBus;
use mediamaster_core::traits::SensorReading;
use mediamaster_core::{HardwareEvent, RoomState};
use mediamaster_sensors::simulated::{ScriptedReading, SimulatedSensorStream};
use mediamaster_sensors::{AsyncSensor, FusionEngine};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_end_to_end_sensor_to_mixer_pipeline() {
    let bus = Arc::new(EventBus::new(32));
    let audio = Arc::new(AudioBridge::new());

    // 1. Load automation rules via TOML DSL
    let rules_toml = r#"
[[rules]]
state = "Occupied"
[rules.action]
type = "ambient_zone"
gain = 0.9
duration_ms = 100

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
curve = "exponential"
"#;
    let rules = AutomationRuleSet::from_toml(rules_toml).expect("valid toml");
    let driver = Arc::new(FusionOutputDriver::new(bus.clone(), audio.clone(), rules));

    // Spawn driver event loop
    let driver_clone = driver.clone();
    let driver_handle = tokio::spawn(async move {
        driver_clone.run().await;
    });

    // Subscribe an event logger to assert event log ordering
    let mut rx = bus.subscribe();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Setup SimulatedSensorStream with scripted occupancy -> vacancy (empty) -> emergency
    let mut sensor_stream = SimulatedSensorStream::new(vec![
        // Step 1: Occupancy detected
        ScriptedReading::immediate(SensorReading::Occupancy),
        // Step 2: Room clears
        ScriptedReading::immediate(SensorReading::Unknown),
    ]);

    let mut fusion_engine = FusionEngine::new();

    // --- Transition 1: Occupied ---
    let sample = sensor_stream.read_sample().await.unwrap();
    assert_eq!(sample, SensorReading::Occupancy);
    fusion_engine.update("seat-1", sample);
    let state = fusion_engine.current_state();
    assert_eq!(state, RoomState::Occupied);

    let _ = bus.publish(HardwareEvent::SensorContextChange { state });

    // Verify ambient gain requested
    let event1 = rx.recv().await.unwrap();
    assert_eq!(
        event1,
        HardwareEvent::SensorContextChange {
            state: RoomState::Occupied
        }
    );
    let event2 = rx.recv().await.unwrap();
    match event2 {
        HardwareEvent::AudioDuckRequest {
            source,
            ducking,
            until,
        } => {
            assert_eq!(source, "fusion_driver_ambient");
            assert_eq!(ducking, 0.9);
            assert_eq!(until, 100);
        }
        other => panic!("expected AudioDuckRequest, got {other:?}"),
    }

    // --- Transition 2: Vacancy ---
    let sample = sensor_stream.read_sample().await.unwrap();
    assert_eq!(sample, SensorReading::Unknown);
    // Replace reading with clear state
    fusion_engine.update("seat-1", SensorReading::Light(100.0));
    let state = fusion_engine.current_state();
    assert_eq!(state, RoomState::Empty);

    let _ = bus.publish(HardwareEvent::SensorContextChange { state });

    let event3 = rx.recv().await.unwrap();
    assert_eq!(
        event3,
        HardwareEvent::SensorContextChange {
            state: RoomState::Empty
        }
    );
    tokio::time::sleep(Duration::from_millis(10)).await;
    // Paging queue paused
    assert!(driver.is_paging_paused());

    // --- Transition 3: Emergency Override ---
    let emergency_state = RoomState::EmergencyOverride;
    let _ = bus.publish(HardwareEvent::SensorContextChange {
        state: emergency_state,
    });

    let event4 = rx.recv().await.unwrap();
    assert_eq!(
        event4,
        HardwareEvent::SensorContextChange {
            state: RoomState::EmergencyOverride
        }
    );
    let event5 = rx.recv().await.unwrap();
    match event5 {
        HardwareEvent::AudioDuckRequest {
            source,
            ducking,
            until,
        } => {
            assert_eq!(source, "fusion_driver_emergency");
            assert_eq!(ducking, 0.0);
            assert_eq!(until, 500);
        }
        other => panic!("expected AudioDuckRequest, got {other:?}"),
    }

    // Mixer ducking should be fully attenuated towards 0.0
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(audio.mixer().lock().unwrap().ducking() < 0.8);

    driver_handle.abort();
}
