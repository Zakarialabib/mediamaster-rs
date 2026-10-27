![CI](https://github.com/Zakarialabib/mediamaster-rs/actions/workflows/ci.yml/badge.svg)
![crates.io](https://img.shields.io/crates/v/mediamaster-core)
![crates.io](https://img.shields.io/crates/v/mediamaster-audio)
![crates.io](https://img.shields.io/crates/v/mediamaster-rf)
![crates.io](https://img.shields.io/crates/v/mediamaster-sensors)

# mediamaster-rs

> Shared hardware media vocabulary for Rust sign-age & streamer apps.

`mediamaster-rs` is a small workset of **orthogonal, runtime-free-core** Rust crates
that share one vocabulary. Each crate solves a single domain. Nothing here is a
UI, a launcher, or a hardware driver — it is the vocabulary that the call and the
phone companion share.

## Workspace layout

```text
mediamaster-rs/
├── Cargo.toml
└── crates/
    ├── mediamaster-core/  # events, traits, shared types
    ├── mediamaster-audio/ # PCM mixing, ducking, cpal sink
    ├── mediamaster-rf/    # SDR IQ pipeline, FFT detection, protocol parsing
    └── mediamaster-sensors/ # sensor polling, debouncing, context fusion
```

## Crate matrix

| Crate | Purpose | Key dependencies |
|-------|---------|------------------|
| `mediamaster-core` | Shared event bus, traits, types | `tokio`, `serde` |
| `mediamaster-audio` | Lock-free PCM mixing, ducking, routing | `mediamaster-core`, `cpal`, `ringbuf` |
| `mediamaster-rf` | SDR IQ pipeline, FFT detection, protocol parsing | `mediamaster-core`, `rustfft`, `nom`, `num-complex` |
| `mediamaster-sensors` | Async sensor polling, debouncing, context fusion | `mediamaster-core`, `embedded-hal-async`, `tokio` |

## Crates.io

| Crate | Version |
|-------|---------|
| [mediamaster-core](https://crates.io/crates/mediamaster-core) | ![0.1.1](https://img.shields.io/crates/v/mediamaster-core) |
| [mediamaster-audio](https://crates.io/crates/mediamaster-audio) | ![0.1.1](https://img.shields.io/crates/v/mediamaster-audio) |
| [mediamaster-rf](https://crates.io/crates/mediamaster-rf) | ![0.1.0](https://img.shields.io/crates/v/mediamaster-rf) |
| [mediamaster-sensors](https://crates.io/crates/mediamaster-sensors) | ![0.1.1](https://img.shields.io/crates/v/mediamaster-sensors) |

## How a call uses these crates

Each crate is a **trait library**. The call application (or any host that wants to
use mediamaster-rs) implements the traits on its hardware adapters and wires the
crates together. Nothing in this workspace is tied to one application.

For example, a TV-signage Tauri app that wants to play audio while detecting motion
works like this:

```text
┌─ TV app (Tauri) ────────────────────────────────┬─ Phone app (Android) ──┐
│  add `AudioSource` adapter for TV audio         │  show signage frames   │
│  add `AsyncSensor` for motion/light             │  subscribe to events   │
│  call `Mixer::mix_step()` every frame           │  render signage    │
└─────────────────────────────────────────────────┴──────────────────────────┘
```

## Integration pattern

```rust
use mediamaster_core::traits::{AudioSource, AsyncSensor};
use mediamaster_audio::Mixer;
use mediamaster_sensors::FusionEngine;

// 1. Implement the traits on your host (the call or the phone app).
//    The calls are *pure*: no hardware handling inside mediamaster-rs itself.

// 2. Combine them in your application.
let mut engine = FusionEngine::new();
let mut mixer = Mixer::new();
```

## Design principles

- **No runtime dependency** in `core` beyond `tokio` + `serde` + `thiserror`.
- **Zero-alloc hot paths**: `AudioSource::read_pcm` writes into a caller-supplied buffer.
- **`async fn` in traits** throughout (Rust 1.75+ native).
- **Priority-ordered** sensor fusion (motion > proximity > light) so map iteration order never affects output.

## Docs

- [architecture.md](docs/architecture.md) — crate-by-crate architecture
- [docs/masteraudio-rs.md](docs/masteraudio-rs.md) — original design notes
- [CHANGELOG.md](CHANGELOG.md) — release history
