# Changelog

## [0.3.0] — Unreleased (Expansion Target)

<!-- Coordinated 0.3.0 workspace release: probe, codec, agent, + purified core. See docs/RELEASE.md. -->

### Added

- **mediamaster-probe** (`0.3.0`, new crate): Pure-Rust, zero-FFmpeg container & bitstream header inspector (`mp4parse`, `matroska`, `symphonia-core`). Extracts codec profile, level, HDR10/P010 flags, chroma subsampling, and frame rate without spawning `ffmpeg`.
- **mediamaster-codec** (`0.3.0`, new crate): Capability-aware codec slotting — `CodecSlotManager`, `DegradationTier` fallback ladder, `CapsDiscovery` seam (`SoftwareCapsDiscovery` portable fallback). Native MF/NDK actors scaffolded behind `mf`/`mediacodec` features (device-validation pending on Windows/Android).
- **mediamaster-agent** (`0.3.0`, new crate): Live Paging & Sensor Ducking Loop — subscribes to the shared `EventBus` and commands `mediamaster-audio::Mixer` on `RoomState::EmergencyOverride` / live `RadioBurstDetected`. Unites `mediamaster-audio`, `mediamaster-sensors`, `mediamaster-rf`.
- **mediamaster-rag** (`0.1.0`, new crate): Acoustic profiles, codec matrix & sensor-context knowledge store — `CodecKnowledgeStore`, `SceneMemoryStore`, `PlaybookStore` for RAG-based acoustic paging and codec recommendation.
- **mediamaster-engine** (`0.3.0`, new crate): Coordination layer — `AudioBus` (lock-free PCM ring), `TranscodePlan` (capability-decision), `EngineSelector` (backend pick: MF/NDK or software fallback).
- **mediamaster-core** (`0.3.0`): purified — `tokio` stripped, `EventBus` relocated to `mediamaster-bus`, `#![no_std]` + `alloc`. CapsMatrix vocabulary added: `CapsMatrix`, `Codec`, `CodecCaps`, `PixelFormat`, `DegradationTier`, `MaintenanceLease`, `MediaCapabilityBundleV1`, `MediaError` + `Remedy`. `RoomState::EmergencyOverride` added.
- **mediamaster-bus** (`0.3.0`): `EventBus` relocated out of core into its own crate (`tokio` broadcast).

### Changed

- **Directory naming**: crates renamed `probe` → `mediamaster-probe`, `codec` → `mediamaster-codec`, `agent` → `mediamaster-agent` (matching package names and workspace convention).
- **README**: all 11 crates now have a comprehensive README.md in their directory.
- **Docs**: new `docs/` directory with `MEDIA-SCOPE-SPEC.md`, `RELEASE.md`, `MIGRATION.md`, `architecture.md`, `masteraudio-rs.md`.

## [0.2.0] — 2026-11-01

<!-- (unchanged baseline release) -->
Coordinated workspace release (aligned versions across the published crates),
plus the first publish of `mediamaster-bridge`. See [`docs/RELEASE.md`](docs/RELEASE.md).

### Added

- **mediamaster-bridge** (new workspace crate, published at 0.1.0):
  app-integration facade — `AudioBridge`, `SensorBridge`, `RfBridge`, plus
  `EventHandlers` that routes `EventBus` events (duck requests → mixer,
  sensor/RF results → recorded state) and exposes `run(bus)` to
  subscribe/dispatch until the bus is dropped.
- **mediamaster-rf**: `OokDemodulator` — OOK/ASK envelope demodulator (315/433 MHz
  ISM standard) recovering a byte stream from I/Q; three-stage `Pipeline`
  (acquisition → DSP → demod/parse) driven by the caller's tokio runtime.
- **mediamaster-audio**: typed audio zones (`Zone`, `ZoneRouter`, `Zones`) with
  sync-group volume/mute; smooth `DuckingProfile` / `DuckingCurve`
  (instant / linear / exponential).

### Changed

- **mediamaster-rf (breaking)**: `Pipeline::new` now takes an `OokDemodulator`;
  the `samples_to_bytes` placeholder is removed (superseded by the demodulator).
  The default `ProtocolParser for ()` now reports the bytes after the sync word
  as the payload (previously `payload_len = 0`).
- **mediamaster-core (breaking)**: `HardwareEvent::ZoneChanged` variant added
  (exhaustive matches downstream must handle it).

## [0.1.1] — 2026-10-27

### Added

- **mediamaster-core**: `SensorReading::priority()` for fusion ordering; `BoundingBox::area()` helper; `MediamasterError::Other(String)` + `context()` for arbitrary error wrapping; `EventBus::capacity()` + `#[must_use]` on `publish`/`subscribe`; all public types re-exported at crate root (`use mediamaster_core::{EventBus, HardwareEvent, RoomState, Signal, SensorReading, AudioSource, …}`)
- **mediamaster-integration-tests** (new workspace crate, `publish = false`): cross-crate e2e tests (RF detection → EventBus → FusionEngine)
- 3 runnable examples: `mix_two_sine_waves` (audio), `synthetic_detection` (rf), `fusion_walk` (sensors)
- `rust-toolchain.toml` pinning Rust 1.95 + rustfmt + clippy components

### Changed

- **FusionEngine correctness**: motion + low-light now returns `PersonPresent` (previously `Occupied`), matching `RoomState` semantics (`PersonPresent` = occupancy + active human presence)
- `SimpleSignageAudioSource::new` now takes `(stream, label)` for explicit labeling
- `Mixer::set_ducking_profile(DuckingProfile)` added as a direct setter

### Fixed

- `BoundingBox` no longer derives `Eq` (inhabits `f32` fields; NaN breaks `Eq` contract)
- `Signal` and `SensorReading` moved from `traits.rs` to `types.rs` (canonical location); re-exported from `traits` for backward compatibility
- `AudioSink` re-exported from `mediamaster_audio` behind `audio-cpal` feature (was private)

### Architecture & Case Study

- Removed the `mediamaster-simple-signage` workspace crate (it was a wrong placeholder). The real signage applications (`Simple-Signage dekstop windows app` and `companion-app mobile android app`) are external repos that adapt the crates by implementing the `mediamaster-core` traits.
- Updated `README.md` (indie-dev style, general conventions, crates.io + CI badges)
- Updated `docs/architecture.md` and `MIGRATION.md` to reflect the external-app-only architecture
