# Migration & Integration Plan — mediamaster-rs → Simple-Signage

> **Status:** VERIFIED ✓ v1 · 2026-10-27 · Audience: launcher working agent + core team
> **Companion specs:** `masteraudio-rs-2.md` (crate design), `MEDIA-SCOPE-SPEC-2.md` (MediaScope extraction) — both reviewed.
> **Verified baseline (this workspace):** `cargo check --workspace` green; `cargo test --workspace` → **50+ passed**; `cargo fmt --check` clean; `audio-cpal` feature compiles. Toolchain: stable MSVC (`stable-x86_64-pc-windows-msvc`).

## 1. Goal
Take the open-source **`mediamaster-rs`** crate suite (at `C:\laragon\www\mediamaster-rs`) and **integrate the pieces the Simple-Signage launcher actually needs**, reusing the exact adapter model `tv-rs` uses (traits in core, OS backends selected at runtime). First integration = `simple_signage::PluginAudio` bridging the signage Tauri app ↔ the engine crate.

## 2. What we take FROM `mediamaster-rs` (and why)
Orthogonal by design — we do NOT drag SDR into a signage player:

| From crate | Re-use | Drop-in target | Notes |
|---|---|---|---|
| `mediamaster-core` | `EventBus` (broadcast), `AudioSource` trait, `HardwareEvent` | new crate `simple-signage-media` | zero deps → ship-safe |
| `mediamaster-audio` | `Mixer` (SPSC ringbuf), `DuckingCurve/DuckingProfile`, `MediaSourceAdapter` | Tauri audio plugin | cpal sink is **feature-gated** (`audio-cpal`); `ringbuf` for real-time; `decode`/symphonia is a codec fallback, **not** AI |
| `mediamaster-sensors` | `DebouncedSensor`, `FusionEngine`, `AsyncSensor` impls | Tauri sensor plugin | `embedded-hal-async` **optional** (`hw` feature) → stubs on desktop |
| `mediamaster-rf` | (deferred) `IqSource`, `MagnitudeDetector`, `Pipeline` | NOT in v0 (no tuner on signage player). Reuse only if TV/RF is added later. |

> **No AI.** `mediamaster-audio`'s `decode` feature (Symex/Symphonia) is codec fallback only. Complies with the SaaS pivot.

## 3. Current state of this workspace (verified)
```
mediamaster-rs/
├── Cargo.toml                          # virtual workspace, members=["crates/*"], resolver="2"
├── README.md · LICENSE-MIT · LICENSE-APACHE
└── crates/
    ├── mediamaster-core/               # EventBus, AudioSource, AsyncSensor, HardwareEvent, types
    ├── mediamaster-audio/              # Mixer (SPSC ringbuf), ducking, cpal sink (audio-cpcap), adapter
    ├── mediamaster-rf/                 # IQ pipeline, MagnitudeDetector, parser
    ├── mediamaster-sensors/            # DebouncedSensor, FusionEngine
    └── mediamaster-simple-signage/     # NEW bridge crate (below) — verified ✓
```
**Bridge crate `mediamaster-simple-signage`** (this repo) — the library-side seam for the launcher:
- `SimpleSignageAudioStream` trait: the **app-side** contract (`read_pcm`, `label`, `is_priority`).
- `SimpleSignageAudioSource : AudioSource`: adapts any `SimpleSignageAudioStream` to the engine's core trait (wraps `Box<dyn SimpleSignageAudioStream>`). `new(stream, label)` takes an explicit label.
- `AudioEngine`: owns a `Mixer`; `add_source` / `add_signage_source` / `signage_stream` / `set_ducking` / `ducking` / `mix_step` / `subscribe` / `process_events`. Behind the `audio-cpal` feature: `into_cpal_sink()` hands the `Mixer` to `AudioSink::start`.
- Event-bus integration: `subscribe(&bus)` + `process_events()` drains `AudioDuckRequest` events and updates ducking before each `mix_step`.
- Tests: 4 unit + 2 adapter integration + 3 event-ducking integration + 1 doc-test = 10 ✓.

## 4. How Simple-Signage currently does audio (Point A)
From `MEDIA-SCOPE-SPEC-2.md` §2.1 + repo grep:
- `src-tauri/Cargo.toml` exposes the media plugin via `media = { path = "../crates/media", features = ["ipc"], optional = true }`.
- Live audio surface: `crates/media/src/audio.rs` — **Windows-only** (`cfg(windows)` + `windows 0.62` `Devices_Enumeration`/`Media_Devices`): endpoint enumeration, per-app volume, `MediaPlayer`/`AudioGraph`/SMTC behind the `windows-media` feature. Non-Windows = **graceful stubs**.
- `src-tauri/crates/` currently holds only `cms-ingest-core` + `platform-mcp-client`.

So the app has **audio device control** but no **mixing/ducking engine** — that is exactly the gap `mediamaster-audio` fills.

## 5. Integration surface (the contract)
Mirror `tv-rs` (`crates/{agent,app,cec,cli,core,mdc,media,power,roku,scan,webos}`): traits in core, OS backend chosen at runtime.

### 5.1 Library side — already in this workspace ✓
`mediamaster-simple-signage` exposes:
```rust
pub trait SimpleSignageAudioStream: Send + Sync {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize;   // normalized -1.0..=1.0
    fn label(&self) -> &str;
    fn is_priority(&self) -> bool { false }            // alerts/TTS
}
impl AudioSource for SimpleSignageAudioSource { ... }  // adapts the stream
pub struct AudioEngine { ... }                          // Mixer + ducking + mix_step
```
Dependency line for the app crate:
```toml
# Simple-Signage/src-tauri/crates/simple-signage-media/Cargo.toml
[dependencies]
mediamaster-core   = { path = "../../../../mediamaster-rs/crates/mediamaster-core" }
mediamaster-audio  = { path = "../../../../mediamaster-rs/crates/mediamaster-audio" }
mediamaster-simple-signage = { path = "../../../../mediamaster-rs/crates/mediamaster-simple-signage" }
```
> Path is `../../../../mediamaster-rs` from `crates/simple-signage-media/` (up: media → crates → src-tauri → Simple-Signage → www). Confirm exact depth on host; prefer pinning mediamaster-rs as a git dep / `[patch]` once the repo is published.

### 5.2 App side — Phase 1 (launcher agent)
A new Tauri plugin `crates/simple-signage-media` (carve from `mediamaster-core+audio+sensors`):
- `src/plugin_audio.rs` — `#[derive(TauriPlugin)] struct PluginAudio`; commands:
  `audio/play_alert`, `audio/stop_alert`, `audio/set_ducking {level}`, `audio/mix_step` (debug probe).
- `src/adapter.rs` — `impl SimpleSignageAudioStream for MediaPlayerStream` wrapping `crates/media/audio.rs` `MediaPlayer` (Windows) / stubs (other OS). Priority alerts (doorbell/TTS) = `is_priority() == true`.
- `src/sensor_plugin.rs` — `mediamaster-sensors::FusionEngine` over `HardwareEvent::Motion` / light. (Phase 2)
- `src-tauri` `Cargo.toml`: add `simple-signage-media = { path = "../crates/simple-signage-media", optional = true }`; enable `audio-cpal` only under `target_os = "windows"`.
- EventBus wiring: subscribe to `HardwareEvent` (e.g. motion → `AudioDuckRequest`-equivalent) and call `engine.set_ducking(...)` (§6.4 duck-on-event).

### 5.3 Reuse model (tv-rs style)
`tv-rs`: `core` defines `Tuner`/traits; `windows`/`linux`/`macos` impl them; app selects at runtime. Here: `mediamaster-core` defines `AudioSource`/`AsyncSensor`; the app's `PluginAudio`/`PluginSensors` impl `SimpleSignageAudioStream`. **No backwards-compat shim** — the old `media` plugin's mixing-free path is replaced entirely (deprecate fully per user rule).

## 6. Backlog
- [x] Phase 0 — `mediamaster-rs` crate suite built + verified (core/audio/rf/sensors).
- [x] Phase 0.5 — `mediamaster-simple-signage` bridge crate (adapter + engine) built + tested.
- [ ] Phase 1 — `simple-signage-media` Tauri plugin (Section 5.2) — **handoff to launcher agent**.
- [ ] Phase 2 — Sensor plugin (`mediamaster-sensors`) + EventBus duck-on-event.
- [ ] Phase 3 — RF only if TV/RF hardware lands.
- [ ] Phase 4 — publish `mediamaster-rs` to crates.io + GitHub repo + CI (follow-up task).
