use mediamaster_audio::Mixer;
use mediamaster_core::caps::{CapsMatrix, Codec, CodecCaps, DegradationTier, PixelFormat};
use mediamaster_rag::{
    CodecCorpusEntry, CodecKnowledgeStore, CodecQuery, DegradationAction, HardwareFingerprint,
    MediamasterRagFacade, OptimalAudioState, SceneMemoryStore, SceneQuery, SceneRecord,
    SensorVector, StarvationTelemetry, StreamDescriptor, StreamLayout, TemporalMetadata,
    TriggerPattern,
};

#[test]
fn test_predictive_degradation_wmf_8gb_vram() {
    let mut store = CodecKnowledgeStore::new(10);
    let fingerprint =
        HardwareFingerprint::new("Windows", "MediaFoundation_D3D11", 8192, "31.0.101");

    // CapsMatrix for host
    let caps = CapsMatrix {
        codecs: vec![CodecCaps {
            codec: Codec::Hevc,
            profile: "Main10".into(),
            level: "5.1".into(),
            max_width: 3840,
            max_height: 2160,
            max_fps: 60,
            pixel_formats: vec![PixelFormat::P010, PixelFormat::Nv12],
            hw_accelerated: true,
        }],
        max_hw_decoder_instances: 3,
        max_hw_encoder_instances: 1,
        verified_on_hardware: true,
    };

    // Historical telemetry showing starvation when running 4x 4K60 HEVC on 8GB VRAM
    let layout_4k_4x = StreamLayout::new(vec![
        StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
        StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
        StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
        StreamDescriptor::new(Codec::Hevc, 3840, 2160, 60, 10),
    ]);

    let starvation_telemetry = StarvationTelemetry {
        dropped_frames_per_sec: 18.5,
        jitter_ms: 45.2,
        decoder_starvation_events: 12,
        surface_allocation_failures: 1,
    };

    let degradation_recs = vec![
        DegradationAction {
            stream_index: 2,
            target_tier: DegradationTier::Tier2HwByteBuffer,
            substream_target: Some(StreamDescriptor::new(Codec::H264, 1920, 1080, 60, 8)),
            reason: "Step down to 1080p SubStream to prevent VRAM allocation starvation".into(),
        },
        DegradationAction {
            stream_index: 3,
            target_tier: DegradationTier::Tier2HwByteBuffer,
            substream_target: Some(StreamDescriptor::new(Codec::H264, 1920, 1080, 60, 8)),
            reason: "Step down to 1080p SubStream to prevent VRAM allocation starvation".into(),
        },
    ];

    store.record_telemetry(CodecCorpusEntry {
        fingerprint: fingerprint.clone(),
        capability_bundle: caps.clone(),
        observed_layout: layout_4k_4x.clone(),
        telemetry: starvation_telemetry,
        recommended_degradations: degradation_recs,
    });

    // Query with a 4-panel video wall request
    let query = CodecQuery {
        fingerprint,
        capability_bundle: Some(caps),
        proposed_layout: layout_4k_4x,
    };

    let recommendation = store.query_codec_playbook(&query).expect("query playbook");

    assert!(recommendation.requires_degradation);
    assert_eq!(recommendation.recommended_actions.len(), 2);
    assert_eq!(recommendation.recommended_actions[0].stream_index, 2);
    assert_eq!(recommendation.recommended_actions[1].stream_index, 3);
    assert_eq!(
        recommendation.recommended_actions[0].substream_target,
        Some(StreamDescriptor::new(Codec::H264, 1920, 1080, 60, 8))
    );
}

#[test]
fn test_atmosphere_harmonization_closing_time() {
    let mut store = SceneMemoryStore::new(10);

    // Profile 1: Morning retail rush
    store.record_scene(SceneRecord {
        sensor_vector: SensorVector::new(0.85, 2.0, 450.0, 5.0, 15.0),
        temporal: TemporalMetadata::new(1, 10, false, "Standard"),
        playbook: mediamaster_rag::AtmospherePlaybook {
            scene_label: "MorningRush".into(),
            audio_state: OptimalAudioState {
                master_volume_db: 0.0,
                ducking_target_gain: 0.1,
                ducking_hold_ms: 400,
                acoustic_preset: "BrightUpbeat".into(),
                crossfade_duration_ms: 1000,
            },
            match_confidence: 1.0,
            description: "High motion, bright retail morning".into(),
        },
    });

    // Profile 2: Closing time (low lux + low motion density)
    store.record_scene(SceneRecord {
        sensor_vector: SensorVector::new(0.05, 8.5, 30.0, -1.0, 0.5),
        temporal: TemporalMetadata::new(1, 22, false, "Standard"),
        playbook: mediamaster_rag::AtmospherePlaybook {
            scene_label: "ClosingTimeCleanup".into(),
            audio_state: OptimalAudioState {
                master_volume_db: -12.0,
                ducking_target_gain: 0.5,
                ducking_hold_ms: 1200,
                acoustic_preset: "LowAmbientWarm".into(),
                crossfade_duration_ms: 5000,
            },
            match_confidence: 1.0,
            description: "Evening wind-down with relaxed ducking and lower master volume".into(),
        },
    });

    // Current query: Late evening, low light, very little motion
    let query = SceneQuery {
        current_sensors: SensorVector::new(0.08, 7.8, 35.0, 0.0, 1.0),
        current_temporal: TemporalMetadata::new(1, 21, false, "Standard"),
    };

    let result = store
        .query_scene_memory(&query)
        .expect("query scene memory");

    assert_eq!(result.scene_label, "ClosingTimeCleanup");
    assert_eq!(result.audio_state.master_volume_db, -12.0);
    assert_eq!(result.audio_state.crossfade_duration_ms, 5000);
    assert_eq!(result.audio_state.acoustic_preset, "LowAmbientWarm");
    assert!(result.match_confidence > 0.7);
}

#[test]
fn test_hot_path_isolation_pcm_mixing() {
    // mediamaster-audio's Mixer and AudioSource operate strictly lock-free and allocation-free
    // on the render thread. Verify mixer mixes without any RAG dependency.
    let mut mixer = Mixer::new();
    let frames = mixer.mix();
    assert_eq!(frames.len(), 0);

    // Ducking profile ease in mixer operates purely on branchless IIR smoother
    mixer.set_ducking(0.5);
    assert_eq!(mixer.ducking(), 0.5);
}

#[test]
fn test_bounded_memory_eviction() {
    let mut store = SceneMemoryStore::new(2);

    for i in 0..5 {
        store.record_scene(SceneRecord {
            sensor_vector: SensorVector::new(i as f32 * 0.1, 5.0, 100.0, 0.0, 0.0),
            temporal: TemporalMetadata::new(1, 12, false, "Standard"),
            playbook: mediamaster_rag::AtmospherePlaybook {
                scene_label: format!("Scene_{i}"),
                audio_state: OptimalAudioState {
                    master_volume_db: 0.0,
                    ducking_target_gain: 0.0,
                    ducking_hold_ms: 300,
                    acoustic_preset: "Default".into(),
                    crossfade_duration_ms: 1000,
                },
                match_confidence: 1.0,
                description: "".into(),
            },
        });
    }

    // Capacity was 2, so only 2 records should remain
    assert_eq!(store.len(), 2);
}

#[test]
fn test_facade_and_context_envelope() {
    let facade = MediamasterRagFacade::new(5, 5);

    let envelope = facade.build_context_envelope(
        1700000000000,
        None,
        None,
        Some((&TriggerPattern::EmergencyOverride, None)),
    );

    assert_eq!(envelope.source, "mediamaster");
    assert_eq!(envelope.timestamp_ms, 1700000000000);
    assert!(envelope.paging_context.is_some());
    assert!(envelope.prompt_summary.contains("PAGING"));
}
