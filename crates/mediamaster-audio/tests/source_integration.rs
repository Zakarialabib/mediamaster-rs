//! Integration tests for the production-grade audio source layer:
//! 1. Sine wave -> CallbackSource -> Mixer -> captured sink buffer matches within 1e-3 RMS.
//! 2. Resample 48k -> 44.1k preserves amplitude.
//! 3. Paging frame arriving mid-stream ducks music zone by the profile's dB and restores on release.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use mediamaster_audio::ducking::{DuckingCurve, DuckingProfile};
use mediamaster_audio::jitter::{PagingJitterBuffer, PagingSource};
use mediamaster_audio::source::CallbackSource;
#[cfg(feature = "resample")]
use mediamaster_audio::source::ReshapingSource;
use mediamaster_audio::Mixer;
use mediamaster_bus::EventBus;
use mediamaster_core::events::HardwareEvent;
use mediamaster_core::traits::AudioSource;

#[test]
fn test_sine_wave_callback_source_mixer_rms() {
    let sample_rate = 48_000.0f32;
    let frequency = 440.0f32;
    let frame_counter = Arc::new(AtomicU64::new(0));

    // CallbackSource generating pure 440Hz sine wave at 48kHz
    let fc = frame_counter.clone();
    let cb_src = CallbackSource::new("sine-cb", move |buffer: &mut [f32]| {
        let start = fc.fetch_add(buffer.len() as u64, Ordering::Relaxed);
        let delta = frequency / sample_rate * 2.0 * std::f32::consts::PI;
        for (i, slot) in buffer.iter_mut().enumerate() {
            let phase = (start + i as u64) as f32 * delta;
            *slot = phase.sin();
        }
    })
    .unwrap();

    let mut mixer = Mixer::new();
    mixer.add_source(Box::new(cb_src));

    // Mix 4096 samples
    let mut captured = Vec::new();
    while captured.len() < 4096 {
        let chunk = mixer.mix();
        captured.extend_from_slice(&chunk);
    }
    captured.truncate(4096);

    // Theoretical RMS of sine wave sin(2*pi*f*t) is 1.0 / sqrt(2) ≈ 0.70710678
    let theoretical_rms = 1.0f32 / 2.0f32.sqrt();

    let sum_sq: f32 = captured.iter().map(|s| s * s).sum();
    let actual_rms = (sum_sq / captured.len() as f32).sqrt();

    let diff = (actual_rms - theoretical_rms).abs();
    assert!(
        diff < 1e-3,
        "RMS difference was {diff} (actual: {actual_rms}, expected: {theoretical_rms}) which exceeds 1e-3"
    );
}

#[cfg(feature = "resample")]
#[test]
fn test_resample_48k_to_44_1k_preserves_amplitude() {
    struct SineSource {
        freq: f32,
        rate: f32,
        frame: AtomicU64,
    }
    impl AudioSource for SineSource {
        fn read_pcm(&self, buffer: &mut [f32]) -> usize {
            let start = self.frame.fetch_add(buffer.len() as u64, Ordering::Relaxed);
            let delta = self.freq / self.rate * 2.0 * std::f32::consts::PI;
            for (i, slot) in buffer.iter_mut().enumerate() {
                let phase = (start + i as u64) as f32 * delta;
                *slot = phase.sin();
            }
            buffer.len()
        }
        fn label(&self) -> &str {
            "sine-48k"
        }
    }

    let sine = SineSource {
        freq: 1000.0,
        rate: 48_000.0,
        frame: AtomicU64::new(0),
    };

    let reshaper = ReshapingSource::new(sine, 48_000, 44_100, 1, 1).unwrap();

    // Discard initial filter warmup
    let mut warmup = [0.0f32; 4096];
    reshaper.read_pcm(&mut warmup);

    // Read test buffer
    let mut test_buf = [0.0f32; 2048];
    reshaper.read_pcm(&mut test_buf);

    let peak = test_buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(
        (peak - 1.0).abs() < 0.05,
        "resampled peak was {peak}, expected close to 1.0"
    );
}

#[test]
fn test_paging_mid_stream_ducks_music_and_restores() {
    let bus = Arc::new(EventBus::new(16));
    let mut rx = bus.subscribe();

    // Music zone source producing unity amplitude 1.0
    struct MusicSource;
    impl AudioSource for MusicSource {
        fn read_pcm(&self, buffer: &mut [f32]) -> usize {
            for slot in buffer.iter_mut() {
                *slot = 1.0;
            }
            buffer.len()
        }
        fn label(&self) -> &str {
            "music"
        }
    }

    let mut mixer = Mixer::new();
    mixer.add_source(Box::new(MusicSource));

    // Initially mixer output should be at full volume (1.0)
    let mixed = mixer.mix();
    assert!(!mixed.is_empty());
    assert!((mixed[0] - 1.0).abs() < 1e-4);

    // Paging source targeting 20 dB duck (target gain = 0.1)
    let target_gain = 0.1f32; // -20 dB
    let profile = DuckingProfile {
        curve: DuckingCurve::Instant,
        duration_ms: 50,
        target: target_gain,
    };

    let jb = PagingJitterBuffer::new(48_000, 10);
    let paging_src = PagingSource::new(jb, "emergency-paging")
        .unwrap()
        .with_auto_ducking(profile.clone(), bus.clone());

    // Paging frame arrives mid-stream
    paging_src.push(&[0.5; 512]);

    // Check that EventBus received the AudioDuckRequest
    let event = rx.try_recv().expect("expected duck request event");
    if let HardwareEvent::AudioDuckRequest {
        source, ducking, ..
    } = event
    {
        assert_eq!(source, "emergency-paging");
        assert_eq!(ducking, target_gain);
    } else {
        panic!("unexpected event: {:?}", event);
    }

    // Apply the ducking profile to the mixer (simulating mediamaster-agent loop)
    mixer.set_ducking_profile(profile);

    // Mix now: music is ducked by profile target dB
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert!(
        (mixer.ducking() - target_gain).abs() < 1e-3,
        "ducking was {}, expected {}",
        mixer.ducking(),
        target_gain
    );

    let ducked_output = mixer.mix();
    assert!(
        (ducked_output[0] - target_gain).abs() < 0.05,
        "output sample was {}, expected ducked ~{}",
        ducked_output[0],
        target_gain
    );

    // Drain paging source so it finishes and releases
    let mut drain_buf = [0.0f32; 1024];
    let _ = paging_src.read_pcm(&mut drain_buf);
    let _ = paging_src.read_pcm(&mut drain_buf);

    // EventBus receives release request
    let release_event = rx.try_recv().expect("expected release event");
    if let HardwareEvent::AudioDuckRequest {
        source, ducking, ..
    } = release_event
    {
        assert_eq!(source, "emergency-paging");
        assert_eq!(ducking, 1.0);
    } else {
        panic!("unexpected release event: {:?}", release_event);
    }

    // Restore mixer
    mixer.set_ducking(1.0);
    mixer.set_ducking_profile(DuckingProfile {
        curve: DuckingCurve::Instant,
        duration_ms: 0,
        target: 1.0,
    });

    let restored = mixer.mix();
    assert!(
        (restored[0] - 1.0).abs() < 1e-4,
        "restored output was {}, expected 1.0",
        restored[0]
    );
}
