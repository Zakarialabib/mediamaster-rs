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

#[test]
fn test_synthetic_telemetry_flips_refuted_verdict_after_threshold() {
    use mediamaster_core::caps::CapabilityVerdict;
    use mediamaster_core::events::HardwareEvent;
    use mediamaster_rag::{QuirksCorpus, TelemetryIngest};

    let mut store = CodecKnowledgeStore::new(10);
    let mut corpus = QuirksCorpus::new();
    let mut ingest = TelemetryIngest::with_thresholds(3.0, 3_600_000.0);

    let dev = "amlogic-s905x4";
    let codec = Codec::Hevc;

    // Initially no observed caps
    assert!(ingest.get_observed_caps(dev, codec).is_none());

    // 1st starvation event -> Borderline
    let event1 = HardwareEvent::StarvationDetected {
        device_class: dev.to_string(),
        codec,
        width: 3840,
        height: 2160,
        fps: 60,
        concurrent_streams: 2,
    };
    ingest.ingest_event(&event1, 1000, &mut store, &mut corpus);
    let obs1 = ingest.get_observed_caps(dev, codec).expect("observed caps");
    assert_eq!(obs1.verdict, CapabilityVerdict::Borderline);
    assert_eq!(obs1.starvation_events, 1);

    // 2nd starvation event -> still Borderline (threshold is 3.0)
    ingest.ingest_event(&event1, 2000, &mut store, &mut corpus);
    let obs2 = ingest.get_observed_caps(dev, codec).expect("observed caps");
    assert_eq!(obs2.verdict, CapabilityVerdict::Borderline);
    assert_eq!(obs2.starvation_events, 2);

    // 3rd starvation event -> flips to Refuted!
    ingest.ingest_event(&event1, 3000, &mut store, &mut corpus);
    let obs3 = ingest.get_observed_caps(dev, codec).expect("observed caps");
    assert_eq!(obs3.verdict, CapabilityVerdict::Refuted);
    assert_eq!(obs3.starvation_events, 3);
    assert!(obs3.confidence >= 0.5);

    // Verify corpus has recorded this refutation and downgrades 4K60
    let test_caps = CodecCaps {
        codec: Codec::Hevc,
        profile: "Main10".into(),
        level: "5.1".into(),
        max_width: 3840,
        max_height: 2160,
        max_fps: 60,
        pixel_formats: vec![PixelFormat::P010],
        hw_accelerated: true,
    };

    let adjusted = corpus.query(dev, &test_caps).expect("quirk adjusted caps");
    assert_eq!(adjusted.max_width, 1920);
    assert_eq!(adjusted.max_height, 1080);
    assert!(!adjusted.hw_accelerated);
}

#[test]
fn test_quirks_aware_discovery_downgrades_lying_backend() {
    use mediamaster_codec::discovery::CapsDiscovery;
    use mediamaster_core::caps::CapabilityVerdict;
    use mediamaster_rag::{QuirksAwareDiscovery, QuirksCorpus, QuirksEntry};

    // Mock backend that advertises 4K60 HEVC with 4 hardware instances
    struct LyingMockDiscovery;
    impl CapsDiscovery for LyingMockDiscovery {
        fn discover(&self) -> CapsMatrix {
            CapsMatrix {
                codecs: vec![CodecCaps {
                    codec: Codec::Hevc,
                    profile: "Main10".into(),
                    level: "5.1".into(),
                    max_width: 3840,
                    max_height: 2160,
                    max_fps: 60,
                    pixel_formats: vec![PixelFormat::P010],
                    hw_accelerated: true,
                }],
                max_hw_decoder_instances: 4,
                max_hw_encoder_instances: 1,
                verified_on_hardware: false,
            }
        }
    }

    let mut corpus = QuirksCorpus::new();
    let dev = "amlogic-s905x4";
    corpus.insert(QuirksEntry {
        device_class: dev.into(),
        codec: Codec::Hevc,
        max_width: Some(1920),
        max_height: Some(1080),
        max_fps: Some(60),
        max_concurrent_streams: Some(1),
        hw_accelerated: Some(false),
        verdict: CapabilityVerdict::Refuted,
        confidence_penalty: 0.8,
        notes: "Starves under multi-stream 4K; downgraded to 1080p60".into(),
    });

    let discovery = QuirksAwareDiscovery::new(LyingMockDiscovery, corpus, dev);
    let matrix = discovery.discover();

    // Verify 4K60 -> 1080p60 downgrade and HW instance limit clamp
    let hevc = matrix.codec(Codec::Hevc).expect("hevc caps");
    assert_eq!(hevc.max_width, 1920);
    assert_eq!(hevc.max_height, 1080);
    assert_eq!(hevc.max_fps, 60);
    assert!(!hevc.hw_accelerated);
    assert_eq!(matrix.max_hw_decoder_instances, 1);
}

#[test]
fn test_corpus_merge_idempotency_and_commutativity() {
    use mediamaster_core::caps::CapabilityVerdict;
    use mediamaster_rag::{QuirksCorpus, QuirksEntry};

    let entry_a1 = QuirksEntry {
        device_class: "soc-a".into(),
        codec: Codec::Hevc,
        max_width: Some(3840),
        max_height: Some(2160),
        max_fps: Some(60),
        max_concurrent_streams: Some(2),
        hw_accelerated: Some(true),
        verdict: CapabilityVerdict::Borderline,
        confidence_penalty: 0.3,
        notes: "Observation from fleet node 1".into(),
    };

    let entry_a2 = QuirksEntry {
        device_class: "soc-a".into(),
        codec: Codec::Hevc,
        max_width: Some(1920),
        max_height: Some(1080),
        max_fps: Some(30),
        max_concurrent_streams: Some(1),
        hw_accelerated: Some(false),
        verdict: CapabilityVerdict::Refuted,
        confidence_penalty: 0.7,
        notes: "Observation from fleet node 2".into(),
    };

    let entry_b = QuirksEntry {
        device_class: "soc-b".into(),
        codec: Codec::Av1,
        max_width: Some(1920),
        max_height: Some(1080),
        max_fps: Some(60),
        max_concurrent_streams: Some(1),
        hw_accelerated: Some(true),
        verdict: CapabilityVerdict::Confirmed,
        confidence_penalty: 0.1,
        notes: "Observation from fleet node 3".into(),
    };

    // Corpus 1: A1 + B
    let mut c1 = QuirksCorpus::new();
    c1.insert(entry_a1.clone());
    c1.insert(entry_b.clone());

    // Corpus 2: A2
    let mut c2 = QuirksCorpus::new();
    c2.insert(entry_a2.clone());

    // Idempotency: C1.merge(C1) == C1
    let mut c1_self = c1.clone();
    c1_self.merge(c1.clone());
    assert_eq!(c1, c1_self);

    // Commutativity: C1 + C2 == C2 + C1
    let mut merge_1_2 = c1.clone();
    merge_1_2.merge(c2.clone());

    let mut merge_2_1 = c2.clone();
    merge_2_1.merge(c1.clone());

    assert_eq!(merge_1_2, merge_2_1);

    // Verify merged result selected the conservative bounds and higher penalty
    let test_req = CodecCaps {
        codec: Codec::Hevc,
        profile: "".into(),
        level: "".into(),
        max_width: 3840,
        max_height: 2160,
        max_fps: 60,
        pixel_formats: vec![],
        hw_accelerated: true,
    };

    let adj = merge_1_2.query("soc-a", &test_req).expect("adjusted caps");
    assert_eq!(adj.max_width, 1920);
    assert_eq!(adj.max_height, 1080);
    assert_eq!(adj.max_fps, 30);
    assert!(!adj.hw_accelerated);
}

#[test]
fn test_quirks_toml_loading_and_json_redaction_roundtrip() {
    use mediamaster_rag::QuirksCorpus;

    // Load seed TOML file
    let corpus =
        QuirksCorpus::load_toml_file("../../quirks/default.toml").expect("load seed default.toml");
    assert!(corpus.len() >= 2);

    // Export redacted JSON
    let json_str = corpus.export_json().expect("export JSON");
    assert!(json_str.contains("redaction_law"));
    assert!(!json_str.contains("ip_address"));
    assert!(!json_str.contains("mac_address"));
    assert!(!json_str.contains("hostname"));
    assert!(!json_str.contains("serial_number"));

    // Import redacted JSON
    let imported_corpus = QuirksCorpus::import_json(&json_str).expect("import JSON");
    assert_eq!(corpus, imported_corpus);
}
