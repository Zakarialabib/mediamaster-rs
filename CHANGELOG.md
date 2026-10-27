# Changelog

## [0.1.1] — 2026-10-27

### Added
|- **mediamaster-core**: `SensorReading::priority()` for fusion ordering; `BoundingBox::area()` helper; `MediamasterError::Other(String)` + `context()` for arbitrary error wrapping; `EventBus::capacity()` + `#[must_use]` on `publish`/`subscribe`; all public types re-exported at crate root (`use mediamaster_core::{EventBus, HardwareEvent, RoomState, Signal, SensorReading, AudioSource, …}`)
|- **mediamaster-integration-tests** (new workspace crate, `publish = false`): cross-crate e2e tests (RF detection → EventBus → FusionEngine)
|- 3 runnable examples: `mix_two_sine_waves` (audio), `synthetic_detection` (rf), `fusion_walk` (sensors)
|- `rust-toolchain.toml` pinning Rust 1.95 + rustfmt + clippy components
|- CI workflow: fmt + clippy (default + all-features) + test matrix
|- Integration tests: 11 across 4 crates (event_bus, detector, fusion, mixer)

### Changed
|- **FusionEngine correctness**: motion + low-light now returns `PersonPresent` (previously `Occupied`), matching `RoomState` semantics (`PersonPresent` = occupancy + active human presence)
|- `SimpleSignageAudioSource::new` now takes `(stream, label)` for explicit labeling
|- `Mixer::set_ducking_profile(DuckingProfile)` added as a direct setter

### Fixed
|- `BoundingBox` no longer derives `Eq` (inhabits `f32` fields; NaN breaks `Eq` contract)
|- `Signal` and `SensorReading` moved from `traits.rs` to `types.rs` (canonical location); re-exported from `traits` for backward compatibility
|- `AudioSink` re-exported from `mediamaster_audio` behind `audio-cpal` feature (was private)

### Architecture & Case Study
|- **Removed the `mediamaster-simple-signage` workspace crate** (it was a wrong placeholder). The real sign-age applications (`Simple-Signage dekstop windows app` and `companion-app mobile android app`) are external repos that adapt the crates by implementing the `mediamaster-core` traits.
|- Updated `README.md` (indie-dev style, general conventions, crates.io + CI badges)
|- Updated `docs/architecture.md` and `MIGRATION.md` to reflect the external-app-only architecture