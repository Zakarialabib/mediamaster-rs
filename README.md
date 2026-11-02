![CI](https://github.com/Zakarialabib/mediamaster-rs/actions/workflows/ci.yml/badge.svg)
![crates.io](https://img.shields.io/crates/v/mediamaster-core)
![crates.io](https://img.shields.io/crates/v/mediamaster-audio)
![crates.io](https://img.shields.io/crates/v/mediamaster-rf)
![crates.io](https://img.shields.io/crates/v/mediamaster-sensors)
![crates.io](https://img.shields.io/crates/v/mediamaster-bridge)

# mediamaster-rs

> Shared hardware media vocabulary for Rust signage & streamer apps.

`mediamaster-rs` is a small workspace of **orthogonal, runtime-free-core** Rust
crates that share one vocabulary. Each crate solves a single domain. Nothing
here is a UI, a launcher, or a hardware driver — it is the vocabulary that the
desktop call app and the phone companion app share.

## Workspace layout

```text
mediamaster-rs/
├── Cargo.toml
└── crates/
    ├── mediamaster-core/             # shared event bus, traits, types, CapsMatrix
    ├── mediamaster-audio/            # PCM mixing, ducking, zones, cpal sink
    ├── mediamaster-rf/               # SDR IQ pipeline, FFT, OOK demod, parsing
    ├── mediamaster-sensors/          # sensor polling, debouncing, context fusion
    ├── mediamaster-bridge/           # app-integration facade (unpublished)
    ├── mediamaster-probe/            # pure-Rust container & bitstream inspector
    ├── mediamaster-codec/            # CapsMatrix consumption, HW slot budgets, MF/NDK
    ├── mediamaster-agent/            # live paging & sensor ducking reconciliation loop
    ├── mediamaster-rag/              # acoustic profiles, codec matrix & sensor-context knowledge store
    ├── mediamaster-engine/           # AudioBus, TranscodePlan, EngineSelector coordination layer
    └── mediamaster-integration-tests # cross-crate tests (unpublished)
```

## Crate matrix

| Crate | Purpose | Key dependencies |
|-------|---------|------------------|
| `mediamaster-core` | Shared event bus, traits, types, CapsMatrix | `serde`, `thiserror` (no tokio, no_std) |
| `mediamaster-bus` | Async `EventBus` (tokio broadcast) + `EventPublisher` trait | `tokio` |
| `mediamaster-audio` | Lock-free PCM mixing, ducking, zones, routing | `mediamaster-core`, `ringbuf`; optional `cpal` (`audio-cpal`) |
| `mediamaster-rf` | SDR IQ pipeline, FFT detection, OOK demod, protocol parsing | `mediamaster-core`, `rustfft`, `nom`, `num-complex` |
| `mediamaster-sensors` | Async sensor polling, debouncing, context fusion | `mediamaster-core`, `tokio`; optional `embedded-hal-async` (`hw`) |
| `mediamaster-bridge` | Unified facade for external apps + `EventBus` routing | `mediamaster-core`, `mediamaster-audio`, `mediamaster-rf`, `mediamaster-sensors` |
| `mediamaster-probe` | Pure-Rust container & bitstream header inspector (MP4/MKV/WebM) | `thiserror` |
| `mediamaster-codec` | CapsMatrix consumption, HW slot budgets, MF/NDK actor scaffolds | `mediamaster-core` |
| `mediamaster-agent` | Live paging & sensor ducking reconciliation loop | `mediamaster-core`, `-bus`, `-audio`, `-sensors`, `-rf`, `-bridge` |
| `mediamaster-rag` | Acoustic profiles, codec matrix & sensor-context knowledge store | `mediamaster-core` |
| `mediamaster-engine` | AudioBus, TranscodePlan, EngineSelector coordination layer | `mediamaster-core`, `-bus`, `-codec`, `-audio`, `ringbuf` |

`mediamaster-bridge` and `mediamaster-integration-tests` are `publish = false`
— they exist for the in-repo apps/tests and are not released to crates.io.

## Crates.io

| Crate | Version |
|-------|---------|
| [mediamaster-core](https://crates.io/crates/mediamaster-core) | ![0.3.0](https://img.shields.io/crates/v/mediamaster-core) |
| [mediamaster-bus](https://crates.io/crates/mediamaster-bus) | ![0.3.0](https://img.shields.io/crates/v/mediamaster-bus) |
| [mediamaster-audio](https://crates.io/crates/mediamaster-audio) | ![0.2.0](https://img.shields.io/crates/v/mediamaster-audio) |
| [mediamaster-rf](https://crates.io/crates/mediamaster-rf) | ![0.2.0](https://img.shields.io/crates/v/mediamaster-rf) |
| [mediamaster-sensors](https://crates.io/crates/mediamaster-sensors) | ![0.2.0](https://img.shields.io/crates/v/mediamaster-sensors) |
| [mediamaster-bridge](https://crates.io/crates/mediamaster-bridge) | ![0.1.0](https://img.shields.io/crates/v/mediamaster-bridge) |
| [mediamaster-probe](https://crates.io/crates/mediamaster-probe) | ![0.1.0](https://img.shields.io/crates/v/mediamaster-probe) |
| [mediamaster-codec](https://crates.io/crates/mediamaster-codec) | ![0.1.0](https://img.shields.io/crates/v/mediamaster-codec) |
| [mediamaster-agent](https://crates.io/crates/mediamaster-agent) | ![0.1.0](https://img.shields.io/crates/v/mediamaster-agent) |
| [mediamaster-rag](https://crates.io/crates/mediamaster-rag) | ![0.1.0](https://img.shields.io/crates/v/mediamaster-rag) |
| [mediamaster-engine](https://crates.io/crates/mediamaster-engine) | ![0.1.0](https://img.shields.io/crates/v/mediamaster-engine) |

The 0.2.0 release is live on crates.io — see [`docs/RELEASE.md`](docs/RELEASE.md). The `0.3.0` expansion (probe, codec, agent) is shipping next; see [`docs/RELEASE.md`](docs/RELEASE.md) for the coordinated publish plan.

"

## How an app uses these crates

Each crate is a **trait library**. The app (a Tauri desktop player, an Android
phone app, or any host) implements the traits on its hardware adapters and wires
the crates together. Nothing in this workspace is tied to one application.

```text
┌─ Desktop call app (Tauri) ────────────────┬─ Phone companion (Android) ─┐
│  impl `AudioSource` for TV audio          │  show signage frames        │
│  impl `AsyncSensor` for motion/light      │  subscribe to events        │
│  call `Mixer::mix()` every frame          │  render signage             │
│  run `EventHandlers::run(bus)` to route   │                             │
└───────────────────────────────────────────┴─────────────────────────────┘
```

## Integration pattern

```rust
use mediamaster_core::traits::{AudioSource, AsyncSensor};
use mediamaster_audio::Mixer;
use mediamaster_sensors::FusionEngine;

// 1. Implement the traits on your host (the app's hardware adapters).
//    The crates are *pure*: no hardware handling inside mediamaster-rs itself.

// 2. Combine them in your application.
let mut engine = FusionEngine::new();
let mut mixer = Mixer::new();
```

## Design principles

- **No runtime dependency** in `core` beyond `tokio` + `serde` + `thiserror`.
- **Zero-alloc hot paths**: `AudioSource::read_pcm` writes into a caller-supplied buffer.
- **`async fn` in traits** throughout (Rust 1.75+ native).
- **Priority-ordered** sensor fusion (motion > proximity > light) so map iteration order never affects output.
- **App-owned wiring**: the `EventBus` loop and hardware live in the app; the crates expose traits + facades.

## Docs

- [architecture.md](docs/architecture.md) — crate-by-crate architecture
- [masteraudio-rs.md](docs/masteraudio-rs.md) — design notes & backlog
- [RELEASE.md](docs/RELEASE.md) — release & crates.io publishing plan
- [MEDIA-SCOPE-SPEC.md](docs/MEDIA-SCOPE-SPEC.md) — spec for the future `media-scope` extraction
- [CHANGELOG.md](CHANGELOG.md) — release history
