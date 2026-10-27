# Changelog

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