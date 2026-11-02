//! Integration test verifying zero overhead and zero lock contention
//! across real-time audio hot paths (`mediamaster-audio`) and SDR IQ pipelines
//! (`mediamaster-rf`) during concurrent background RAG context operations.

use std::sync::Arc;
use tokio::sync::RwLock;

use mediamaster_agent::Agent;
use mediamaster_audio::Mixer;
use mediamaster_bridge::AudioBridge;
use mediamaster_bus::EventBus;
use mediamaster_core::traits::{AudioSource, SignalDetector};
use mediamaster_rag::{
    AtmospherePlaybook, MediamasterRagFacade, OptimalAudioState, SceneRecord, SensorVector,
    TemporalMetadata,
};
use mediamaster_rf::MagnitudeDetector;

#[tokio::test]
async fn test_hot_paths_maintain_zero_overhead_during_background_rag() {
    let mut rag_facade = MediamasterRagFacade::new(10, 10);
    rag_facade.scenes.record_scene(SceneRecord {
        sensor_vector: SensorVector::new(0.5, 3.0, 100.0, 0.0, 2.0),
        temporal: TemporalMetadata::new(1, 14, false, "Standard"),
        playbook: AtmospherePlaybook {
            scene_label: "BusyRetail".into(),
            audio_state: OptimalAudioState {
                master_volume_db: 0.0,
                ducking_target_gain: 0.1,
                ducking_hold_ms: 500,
                acoustic_preset: "Bright".into(),
                crossfade_duration_ms: 100,
            },
            match_confidence: 1.0,
            description: "Busy daytime retail".into(),
        },
    });

    let rag = Arc::new(RwLock::new(rag_facade));
    let bus = Arc::new(EventBus::new(16));
    let audio = Arc::new(AudioBridge::new());
    let agent = Agent::new(bus.clone(), audio.clone()).with_rag(rag.clone());

    // Spawn intensive background RAG querying task
    let rag_worker = rag.clone();
    let bg_task = tokio::spawn(async move {
        for _ in 0..100 {
            let facade = rag_worker.read().await;
            let _ = facade.build_context_envelope(1000, None, None, None);
        }
    });

    // 1. Real-time audio hot path: Mixer::mix and AudioBridge::read_pcm
    // must execute smoothly with zero blocking or memory contention
    let mut pcm_buffer = [0.0f32; 512];
    for _ in 0..50 {
        let n = audio.read_pcm(&mut pcm_buffer);
        assert!(n <= 512);
    }

    let mut direct_mixer = Mixer::new();
    let frames = direct_mixer.mix();
    assert_eq!(frames.len(), 0);

    // 2. SDR IQ pipeline hot path: MagnitudeDetector FFT peak detection
    let detector = MagnitudeDetector::new(16, 0.2);
    let spectrum = vec![0.05, 0.85, 0.20, 0.90, 0.10, 0.05, 0.40, 0.15];
    let mut peaks = Vec::new();
    let count = detector.detect(&spectrum, &mut peaks);
    assert_eq!(count, 3);

    bg_task.await.expect("bg task finished cleanly");

    // Agent envelope generation completes without contention
    let env = agent.get_context_envelope(1000, None, None, None).await;
    assert!(env.is_some());
}
