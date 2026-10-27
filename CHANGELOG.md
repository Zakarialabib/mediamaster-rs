# Changelog

## [0.1.1] — 2026-10-27

### Added
- **mediamaster-core**: `SensorReading::priority()` for fusion ordering; `BoundingBox::area()` helper; `MediamasterError::Other(String)` + `context()` for arbitrary error wrapping; `EventBus::capacity()` + `#[must_use]` on `publish`/`subscribe`; all public types re-exported at crate root (`use mediamaster_core::{EventBus, HardwareEvent, RoomState, Signal, SensorReading, AudioSource, …}`)
- **mediamaster-simple-signage** (new crate, `publish = false`): SimpleSignage adapter — `SimpleSignageAudioStream` trait, `SimpleSignageAudioSource` adapter (`AudioSource` impl), `AudioEngine` with event-bus-driven ducking (`subscribe`, `process_events`, `mix_step`)
- **mediamaster-integration-tests** (new workspace crate, `publish = false`): cross-crate e2e tests (RF detection → EventBus → FusionEngine)
- 3 runnable examples: `mix_two_sine_waves` (audio), `synthetic_detection` (rf), `fusion_walk` (sensors)
- `rust-toolchain.toml` pinning Rust 1.95 + rustfmt + clippy components
- CI workflow: fmt + clippy (default + all-features) + test matrix
- Integration tests: 11 across 4 crates (event_bus, detector, fusion, mixer)

### Changed
- **FusionEngine correctness**: motion + low-light now returns `PersonPresent` (previously `Occupied`), matching `RoomState` semantics (`PersonPresent` = occupancy + active human presence)
- `SimpleSignageAudioSource::new` now takes `(stream, label)` for explicit labeling
- `Mixer::set_ducking_profile(DuckingProfile)` added as a direct setter

### Fixed
- `BoundingBox` no longer derives `Eq` (inhabits `f32` fields; NaN breaks `Eq` contract)
- `Signal` and `SensorReading` moved from `traits.rs` to `types.rs` (canonical location); re-exported from `traits` for backward compatibility
- `AudioSink` re-exported from `mediamaster_audio` behind `audio-cpal` feature (was private)

### Published to crates.io
- `mediamaster-core` 0.1.0 ✓ (republishing 0.1.1 pending)
- `mediamaster-audio` 0.1.0 ✓ (republishing 0.1.1 pending)
- `mediamaster-rf` 0.1.0 ✓ (no changes in 0.1.1)
- `mediamaster-sensors` 0.1.0 ✓ (republishing 0.1.1 pending)

---

## [Unreleased] — Build state established

### Summary
- `cargo check --workspace` — passes
- `cargo clippy --workspace --all-targets -- -D warnings` — passes
- `cargo test --workspace` — 22 tests pass (audio: 7, core: 4, rf: 5, sensors: 6)

### Fixed (18 compilation errors, workspace-wide)

#### mediamaster-core
- `error.rs`: typo `MedimasterError` → `MediamasterError`
- `types.rs`: removed `Eq` from `BoundingBox`, `RadioBurst` (contain `f32`); `RadioBurst.detected_by` `&'static str` → `String` (fixes `Deserialize` lifetime)
- `events.rs`: added `is_empty()` to `EventBus` (clippy `len_without_is_empty`)
- `traits.rs`: removed unused `RoomState` import; `#[allow(async_fn_in_trait)]` on `AsyncSensor`

#### mediamaster-audio
- Created missing `lib.rs` (crate root)
- Rewrote `mixer.rs`: removed `par_iter`/rayon, fixed `HeapRb<f32>` type params, `mix(&mut self)`, `use ringbuf::Rb`
- `ducking.rs`: `dur / 2` → `2.0` (f32 division); derived `PartialEq` on `DuckingCurve`
- `sink.rs`: removed `alloc::vec::Vec` import; `source.rs`: `needless_range_loop` fix
- Per-crate README updated with accurate API surface

#### mediamaster-rf
- `Cargo.toml`: `num-complex` 0.3 → 0.4 (resolves conflict with rustfft 0.10 transitive dep)
- `lib.rs`: re-export `SignalDetector`, `IqSource`, `ProtocolKind`, `AcquisitionStage`
- `parser.rs`: explicit lifetime `'a` on `parse_sync` for nom error type alignment
- `detector.rs`: `Fft<f32>` (trait-as-type) → `Arc<dyn Fft<f32>>`; added `SignalDetector` impl
- `pipeline.rs`: fixed `MediamasterError` path, `Vec<IqSample>` return type
- Per-crate README updated (`Pipeline` → `AcquisitionStage`)

#### mediamaster-sensors
- Created `sensor.rs`, `debouncer.rs`, `aggregator.rs`, `fusion.rs` (4 modules missing)
- `lib.rs`: fixed duplicate `FusionEngine` re-export
- `fusion.rs`: `current_state` restructured to priority-ordered checks (HashMap iteration order was non-deterministic)
- `iq_source.rs`: `#[allow(async_fn_in_trait)]`

### Added
- `docs/architecture.md` — crate-by-crate architecture reference
- `.gitignore` — Rust + Python + IDE patterns
- `.github/workflows/ci.yml` — CI (check, clippy, test)
- 22 unit tests across 4 crates