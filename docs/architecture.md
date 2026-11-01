# Architecture

## Overview

`mediamaster-rs` provides a shared vocabulary for hardware media systems — a
foundation that sits *alongside* `tv-rs` (broadcast TV) and `cctv-rs`
(security cameras). The signage frontends consume these crates by implementing
their traits; no domain is integrated into the signage app directly.

## Crate dependency graph

```text
mediamaster-core          <- every other crate
   ↑
   |  mediamaster-audio · mediamaster-rf · mediamaster-sensors · mediamaster-bridge
   |
mediamaster-integration-tests   (test-only, depends on core/rf/sensors/audio)
```

`mediamaster-core` is the root: every other crate depends on it. There are
**no cross-dependencies** between the domains — they are orthogonal
capabilities that share the `core` vocabulary.

> `simple-signage` was removed in 2026-10 (the apps are external
> repositories). `mediamaster-bridge` is the app-integration seam: it re-exports
> the domains behind one facade (`AudioBridge`, `SensorBridge`, `RfBridge`,
> `EventHandlers`). `EventHandlers` provides the `EventBus` subscription loop
> (`run`) that routes events to the bridges, or apps can drive `handle_event`
> themselves.

## `mediamaster-core`

Shared types, traits, and the async event bus.

- **`traits`**: `AudioSource` (PCM frame producer), `AsyncSensor` (async sensor
  poll), `SignalDetector` (FFT peak detection). `Signal` and `SensorReading`
  live in `types` (re-exported from `traits` for backward compat).
- **`types`**: `BoundingBox` (with `area()`), `RadioBurst`,
  `Signal` (serde), `AudioTarget`, `RoomState`, `SensorReading` (serde +
  `priority()`).
- **`events`**: `EventBus` (broadcast channel, `#[must_use]` on `publish`/
  `subscribe`), `HardwareEvent` enum (`AudioDuckRequest`, `SensorContextChange`,
  `RadioBurstDetected`, `CctvMotionDetected`, `ZoneChanged`).
- **`error`**: `MediamasterError` — single error type; `Other(String)` +
  `context()` for arbitrary error wrapping.

Key decisions:
- `async fn` in traits (Rust 1.75+ native, behind `#[allow(async_fn_in_trait)]`).
- `RadioBurst.detected_by` is a `String` (not `&'static str`) to be
  `Deserialize`-compatible without `'de: 'static` constraints.
- No `f32` field carries `Eq` (floats are not `Eq`).

## mediamaster-audio

Lock-free PCM mixing with per-source ducking.

- **`AudioSource`** (core trait, implemented here by `MediaSourceAdapter`):
  `read_pcm(&self, &mut [f32]) -> usize` — fill a caller-supplied buffer,
  return frame count. `MediaSourceAdapter` bridges another hardware domain
  (e.g. `tv-rs`) into the mixer; currently a dummy that fills zeros.
- **`Mixer`**: `add_source(Box<dyn AudioSource>)`, `mix(&mut self) -> Vec<f32>`,
  `set_ducking(f32)`, `set_ducking_profile(DuckingProfile)` — sums all sources,
  applies ducking, and drains into a `ringbuf::HeapRb<f32>`.
- **`Sink`** (cpal, feature-gated `audio-cpal`): `LockRead` trait + `AudioSink`
  struct (`start(...)` spawns the OS audio output thread).
- **`DuckingProfile`** / **`DuckingCurve`**: a ducking profile (`curve`,
  `duration_ms`, `target`) applied before writing to the output ring buffer;
  `DuckingCurve` is `Instant` / `Exponential` / `Linear`.
- **Zones** (`zones.rs`): `Zone` (`id`, `name`, `output_device_id`,
  `volume_db`, `muted`, `sync_group`) + the `ZoneRouter` trait, implemented by
  `Zones` — typed audio routing zones with sync-group volume/mute, publishing
  `ZoneChanged` events over the `EventBus`.

## mediamaster-sensors

Async sensor polling with debouncing and context fusion.

- **`DebouncedSensor`**: emits a reading only after it has been stable for
  `window` duration. First call for a reading starts the timer; same reading
  with elapsed window emits; different reading resets the timer.
- **`SensorAggregator`**: `HashMap<String, SensorReading>` — `push(id, reading)`
  replaces the latest value per sensor (`readings()`, `len()`, `is_empty()`).
- **`FusionEngine`**: `current_state() -> RoomState` — priority-ordered checks
  (motion + low-light → `PersonPresent`; motion only → `Occupied`; proximity
  < 10 m → `PersonPresent`; light < 50 lux → `LowLight`; else `Empty`).
- **`SensorBridge`** (`mediamaster-bridge`): wraps `FusionEngine` behind the
  `AsyncSensor` core trait for the apps.

## mediamaster-rf

SDR I/Q pipeline: acquire → FFT → detect → OOK demod → parse.

- **`IqSource`** (trait): `async fn acquire(&self, count: usize) -> Vec<Complex<f32>>`,
  plus `fn sample_rate_hz(&self) -> u32`.
- **`MagnitudeDetector`**: FFT forward transform + magnitude threshold.
  `new(size, threshold)`, `peaks(&[Complex<f32>])`, `detect(...)`.
- **`OokDemodulator`**: OOK/ASK envelope demodulator (the 315/433 MHz ISM-band
  standard) — averages the sample envelope over `samples_per_symbol` and slices
  against `threshold` to recover a byte stream (MSB-first).
- **`ProtocolParser`** (trait): `parse_sync` recognizes sync words (`\xaa\xbb`)
  and extracts `Header` (kind, sync_word, payload_len). The default `()`
  parser reports the bytes after the sync word as the payload.
- **`Pipeline`** — three-stage async pipeline (acquisition → DSP → demod/parse)
  over an `IqSource` + `EventBus`, driven by the caller's runtime. Config:
  `PipelineConfig` (`batch_size`, `fft_size`, `threshold`, `buffer_size`,
  `center_frequency_hz`).

## mediamaster-bridge

Unified facade over the domains for the external apps.

- **`AudioBridge`** — `AudioSource` + `Mixer` (Mutex-wrapped so `read_pcm` takes
  `&self`), ducking-profile passthrough, device enumerators.
- **`SensorBridge`** — `AsyncSensor` over `FusionEngine` + device enumerators.
- **`RfBridge`** — `SignalDetector` over `MagnitudeDetector` + device enumerators.
- **`EventHandlers`** — routes `EventBus` events to the domains: builds a
  `DuckingProfile` from `AudioDuckRequest` and applies it to the `AudioBridge`
  mixer, records the latest `RoomState` (`SensorContextChange`) and `RadioBurst`
  (`RadioBurstDetected`) for the app to poll; `run(bus)` subscribes and
  dispatches until the bus is dropped (`ZoneChanged`/`CctvMotionDetected` are
  no-ops — no zone/CCTV bridge here).

## mediamaster-integration-tests (workspace-only)

Cross-crate integration tests that exercise the shared trait vocabulary
(`mediamaster-core`) and the domain crates (`audio`, `rf`, `sensors`, `bridge`).

|| Tool | Command |
|------|---------|-|
| Build | `cargo check --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Test | `cargo test --workspace` (70 tests) |
| Format | `cargo fmt` |
| Toolchain | 1.95 (per `rust-toolchain.toml`, with `rustfmt` + `clippy`) |
| CI | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (on `stable`) |
