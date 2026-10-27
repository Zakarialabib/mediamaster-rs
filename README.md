# mediamaster-rs

> **Hardware media-masteration ecosystem** — a shared foundation *alongside* `tv-rs`
> (broadcast) and `cctv-rs` (security camera).

The four crates are **orthogonal domains** that share one vocabulary. Nothing here
integrates audio, SDR, or sensors into the signage app directly — `tv-rs`, `cctv-rs`,
and the signage frontends (`signage-master` Windows app, `signage-master-companion`
Android) **adapt** them by implementing this crate's traits.

## Workspace layout

```text
mediamaster-rs/
├── Cargo.toml              # workspace root
└── crates/
    ├── mediamaster-core/     # events, traits, shared types
    ├── mediamaster-audio/    # PCM mixing, ducking, cpal sink
    ├── mediamaster-rf/        # SDR IQ pipeline, FFT detection, protocol parsing
    └── mediamaster-sensors/   # async sensor polling, debouncing, context fusion
```

## Crate matrix

| Crate | Purpose | Key dependencies |
|-------|---------|------------------|
| `mediamaster-core` | Shared event bus, traits, types | `tokio`, `serde` |
| `mediamaster-audio` | Lock-free PCM mixing, ducking, routing | `mediamaster-core`, `cpal`, `ringbuf` |
| `mediamaster-rf` | SDR IQ pipeline, FFT detection, protocol parsing | `mediamaster-core`, `rustfft`, `nom`, `num-complex` |
| `mediamaster-sensors` | Async sensor polling, debouncing, context fusion | `mediamaster-core`, `embedded-hal-async`, `tokio` |

## Build

```sh
cargo check --workspace          # verify compilation
cargo clippy --workspace --all-targets -- -D warnings   # lint
cargo test --workspace           # 22 unit tests
```

## Design principles

- **No runtime dependency** in `core` beyond `tokio` + `serde`.
- **Zero-alloc hot paths**: `AudioSource::read_pcm` writes into a caller-supplied buffer.
- **`async fn` in traits** throughout (Rust 1.75+ native).
- **Lock-free SPSC** ring buffers between SDR read thread and consumer pipeline.
- **Priority-ordered** sensor fusion (motion > proximity > light) so map iteration order never affects output.

## Docs

- [architecture.md](docs/architecture.md) — crate-by-crate architecture
- [docs/masteraudio-rs.md](docs/masteraudio-rs.md) — original design notes
- [docs/MEDIA-SCOPE-SPEC.md](docs/MEDIA-SCOPE-SPEC.md) — scope specification
