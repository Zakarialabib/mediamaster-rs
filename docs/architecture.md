# Architecture

## Overview

`mediamaster-rs` provides a shared vocabulary for hardware media systems — a
foundation that sits *alongside* `tv-rs` (broadcast TV) and `cctv-rs` (security
cameras). The signage frontends consume these crates by implementing their
traits; no domain is integrated into the signage app directly.

## Crate dependency graph

```
core ← audio
core ← rf    ← (nom, rustfft, num-complex 0.4)
core ← sensors ← (embedded-hal-async)
simple-signage → core + audio  (publish = false)
integration-tests → core + audio + rf + sensors  (publish = false)
```

`mediamaster-core` is the root: every other crate depends on it. There are **no
cross-dependencies** between `audio`, `rf`, and `sensors` — they are
orthogonal domains that happen to share the `core` vocabulary.

## `mediamaster-core`

Shared types, traits, and the async event bus.

- **`traits`**: `AudioSource` (PCM frame producer), `AsyncSensor` (async sensor
  poll), `SignalDetector` (FFT peak detection). `Signal` and `SensorReading`
  now live in `types` (re-exported from `traits` for backward compat).
- **`types`**: `BoundingBox` (with `area()`), `RadioBurst`, `Signal` (serde),
  `AudioTarget`, `RoomState`, `SensorReading` (serde + `priority()`).
- **`events`**: `EventBus` (broadcast channel, `#[must_use]` on `publish`/`subscribe`),
  `HardwareEvent` enum (`AudioDuckRequest`, `SensorContextChange`, `RadioBurstDetected`, `CctvMotionDetected`).
- **`error`**: `MediamasterError` — single error type; `Other(String)` +
  `context()` for arbitrary error wrapping.

Key decisions:
- `async fn` in traits (Rust 1.75+ native, behind `#[allow(async_fn_in_trait)]`).
- `RadioBurst.detected_by` is a `String` (not `&'static str`) to be
  `Deserialize`-compatible without `'de: 'static` constraints.
- No `f32` field carries `Eq` (floats are not `Eq`).

## `mediamaster-audio`

Lock-free PCM mixing with per-source ducking.

- **`Source`**: `AudioSource::read_pcm(&self, &mut [f32]) -> usize` — fill a
  caller-supplied buffer, return frame count.
- **`Mixer`**: `add_source`, `mix(&mut self) -> Vec<f32>`, `set_ducking(f32)` —
  summing all sources, applies ducking, and drains into a `ringbuf::HeapRb<f32>`.
- **`Sink`** (cpal, feature-gated): `LockRead` trait + `AudioSink` struct
  for the OS audio output thread.
- **`DuckingProfile`**: linear / exponential / instant curves from 1.0 → target.

`new(4, 0.5)` creates a detector expecting 4-sample FFT windows with a 0.5
magnitude threshold. `peaks(&[Complex<f32>])` runs the FFT and returns per-bin
magnitudes. `detect(&[f32], &mut Vec<Signal>)` filters magnitudes above
threshold and writes `Signal` results.

## `mediamaster-sensors`

Async sensor polling with debouncing and context fusion.

- **`DebouncedSensor`**: emits a reading only after it has been stable for
  `window` duration. First call for a reading starts the timer; same reading
  with elapsed window emits; different reading resets the timer.
- **`SensorAggregator`**: `HashMap<String, SensorReading>` — `update(id, reading)`
  replaces the latest value per sensor.
- **`FusionEngine`**: `current_state() -> RoomState` — priority-ordered checks
  (motion + low-light → `PersonPresent`; motion only → `Occupied`; proximity
  <10 cm → `PersonPresent`; light <50 lux → `LowLight`; else `Empty`).

## `mediamaster-rf`

SDR I/Q pipeline: acquire → detect → decode.

- **`iq_source`**: `IqSource` trait — `fn read(&mut self) -> IqSample` (a
  `Complex<f32>`). `read_async` default uses `tokio::task::spawn_blocking`.
- **`detector`**: `MagnitudeDetector` — FFT forward transform + magnitude
  threshold. Configurable size and threshold.
- **`parser`**: `ProtocolParser` trait — `parse_sync` recognizes sync words
  (`\xaa\xbb`) and extracts `Header` (kind, sync_word, payload_len).
- **`pipeline`**: `AcquisitionStage` — wraps an `IqSource` + `SignalDetector`
  into an async acquisition loop with graceful shutdown via `CancellationToken`.

## `mediamaster-integration-tests` (workspace-only)

Cross-crate integration tests that exercise the shared trait vocabulary
(`mediamaster-core`) and the domain crates (`audio`, `rf`, `sensors`).

| Tool | Command |
|------|---------|
| Build | `cargo check --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Test | `cargo test --workspace` (50+ tests) |
| Format | `cargo fmt` |
| MSRV | Rust 1.95 (per `rust-toolchain.toml`) |
| CI | `cargo clippy --workspace --all-features --all-targets -- -D warnings` |
