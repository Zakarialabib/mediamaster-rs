//! Runnable demo: synthetic tones with live paging and automatic ducking.
//!
//! Demonstrates:
//! 1. Background music playback using `CallbackSource` (440 Hz synthetic tone).
//! 2. Live paging stream via `PagingJitterBuffer` / `PagingSource` (880 Hz beep).
//! 3. Automatic ducking loop integration via `mediamaster-bus`: when paging audio
//!    arrives, the music zone ducks according to the configured `DuckingProfile`
//!    and smoothly restores upon paging stream release.
//!
//! Run with:
//! `cargo run -p mediamaster-audio --example paging_duck`

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use mediamaster_audio::ducking::{DuckingCurve, DuckingProfile};
use mediamaster_audio::jitter::{PagingJitterBuffer, PagingSource};
use mediamaster_audio::source::CallbackSource;
use mediamaster_audio::Mixer;
use mediamaster_bus::EventBus;
use mediamaster_core::events::HardwareEvent;

fn main() {
    println!("=== MediaMaster Audio: Paging Duck Demo ===");

    let sample_rate = 48_000.0f32;
    let bus = Arc::new(EventBus::new(16));
    let mut rx = bus.subscribe();

    // 1. Background Music Zone (440 Hz tone)
    let music_frame = Arc::new(AtomicU64::new(0));
    let mf = music_frame.clone();
    let music_source = CallbackSource::new("background-music", move |buffer: &mut [f32]| {
        let start = mf.fetch_add(buffer.len() as u64, Ordering::Relaxed);
        let delta = 440.0 / sample_rate * 2.0 * std::f32::consts::PI;
        for (i, slot) in buffer.iter_mut().enumerate() {
            let phase = (start + i as u64) as f32 * delta;
            *slot = 0.5 * phase.sin();
        }
    })
    .expect("create music source");

    // 2. Live Paging Zone (880 Hz alert beep) with automatic ducking
    let duck_profile = DuckingProfile {
        curve: DuckingCurve::Linear,
        duration_ms: 150,
        target: 0.1, // -20 dB attenuation
    };

    let jitter_buffer = PagingJitterBuffer::new(48_000, 20);
    let paging_source = Arc::new(
        PagingSource::new(jitter_buffer, "live-paging")
            .expect("create paging source")
            .with_auto_ducking(duck_profile.clone(), bus.clone()),
    );

    let mut mixer = Mixer::new();
    mixer.add_source(Box::new(music_source));
    mixer.add_source(Box::new(PagingSourceRef(paging_source.clone())));

    println!("[Stage 1] Playing background music (gain = 1.0)...");
    for _ in 0..5 {
        let out = mixer.mix();
        let max_val = out.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        println!(
            "  Mixer batch: peak amplitude = {:.4}, ducking = {:.2}",
            max_val,
            mixer.ducking()
        );
    }

    println!("\n[Stage 2] Live paging alert burst arrives!");
    let mut paging_tone = [0.0f32; 1024];
    for (i, s) in paging_tone.iter_mut().enumerate() {
        let phase = i as f32 * (880.0 / sample_rate * 2.0 * std::f32::consts::PI);
        *s = 0.9 * phase.sin();
    }
    paging_source.push(&paging_tone);

    // Event bus notification handled by application loop / agent
    if let Ok(HardwareEvent::AudioDuckRequest {
        source,
        ducking,
        until,
    }) = rx.try_recv()
    {
        println!(
            "  [EventBus] {} requested duck to {:.2} over {}ms",
            source, ducking, until
        );
        mixer.set_ducking_profile(DuckingProfile {
            curve: DuckingCurve::Linear,
            duration_ms: until,
            target: ducking,
        });
    }

    for step in 1..=8 {
        let out = mixer.mix();
        let max_val = out.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        println!(
            "  Ducked batch {}: peak = {:.4}, ducking level = {:.2}",
            step,
            max_val,
            mixer.ducking()
        );
    }

    println!("\n[Stage 3] Paging finishes; stream drains and releases ducking...");
    for _ in 0..10 {
        let _ = mixer.mix();
    }

    if let Ok(HardwareEvent::AudioDuckRequest {
        source, ducking, ..
    }) = rx.try_recv()
    {
        println!(
            "  [EventBus] {} released; restoring volume to {:.2}",
            source, ducking
        );
        mixer.set_ducking(ducking);
        mixer.set_ducking_profile(DuckingProfile {
            curve: DuckingCurve::Linear,
            duration_ms: 50,
            target: ducking,
        });
    }

    let out = mixer.mix();
    let max_val = out.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    println!(
        "  Restored batch: peak = {:.4}, ducking level = {:.2}",
        max_val,
        mixer.ducking()
    );
    println!("\nDemo finished successfully!");
}

struct PagingSourceRef(Arc<PagingSource>);
impl mediamaster_core::traits::AudioSource for PagingSourceRef {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        self.0.read_pcm(buffer)
    }
    fn label(&self) -> &str {
        self.0.label()
    }
}
