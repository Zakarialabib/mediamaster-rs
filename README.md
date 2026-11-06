# Mediamaster Rust

A Rust workspace (Tauri+Vue frontend is separate) of zero-FFmpeg, async-native crates for a
signage broadcasting system. One Rust workspace, many published crates — each with its own
crates.io package and version.

```
mediamaster-rs/
├── Cargo.toml                       # workspace manifest (members = crates/*)
├── rust-toolchain.toml              # pinned Rust 1.95 + rustfmt + clippy
├── README.md                        # this file (workspace overview)
├── CHANGELOG.md                     # version history
├── docs/
│   ├── RELEASE.md                   # coordinated publish workflow (reference)
│   └── MIGRATION.md                 # 0.2.0 → 0.3.0 breaking changes
└── crates/                          # 11 workspace members (see below)
```

## Crate matrix

| Crate | Package | Version | Published | Purpose |
|---|---|---|---|---|
| `mediamaster-core` | `mediamaster-core` | 0.4.0 | ✅ | Pure Rust, `#![no_std]`+`alloc`, `EventBus` relocated, `CapsMatrix` vocabulary, `OKLab/HCT` palette engine |
| `mediamaster-bus` | `mediamaster-bus` | 0.4.0 | ✅ | `EventBus` (`tokio::sync::broadcast`) — the unified app event bus |
| `mediamaster-probe` | `mediamaster-probe` | 0.4.0 | ✅ | Zero-FFmpeg container & bitstream inspector (MP4, MKV, WebM, MPEG-TS, MP3, ADTS/AAC, FLAC, WAV/RF64, JPEG, PNG, GIF, WebP, Annex-B NAL probe) |
| `mediamaster-codec` | `mediamaster-codec` | 0.4.0 | ✅ (portable) | Capability-aware codec slotting, `CapsMatrix`, `DegradationTier` fallback |
| `mediamaster-frame` | `mediamaster-frame` | 0.4.0 | ✅ | Frame and packet vocabulary (`FramePacket`, channels) |
| `mediamaster-audio` | `mediamaster-audio` | 0.4.0 | ✅ | Typed audio zones, `DuckingProfile`, `PagingJitterBuffer` (live paging) |
| `mediamaster-rf` | `mediamaster-rf` | 0.4.0 | ✅ | OOK/ASK demodulator, 3-stage pipeline, real parser |
| `mediamaster-sensors` | `mediamaster-sensors` | 0.4.0 | ✅ | Sensor reading, debounce, fusion context, hardware (`hw`) backends |
| `mediamaster-rag` | `mediamaster-rag` | 0.4.0 | ✅ | Acoustic profiles, codec matrix, sensor-context knowledge store |
| `mediamaster-bridge` | `mediamaster-bridge` | 0.4.0 | ✅ | App-integration facade: `AudioBridge`, `SensorBridge`, `RfBridge`, enumerators, event bus |
| `mediamaster-engine` | `mediamaster-engine` | 0.4.0 | ✅ | Coordination layer: `AudioBus`, `TranscodePlan`, `EngineSelector` |
| `mediamaster-agent` | `mediamaster-agent` | 0.4.0 | ✅ | Live Paging & Sensor Ducking Loop — control plane |
| `mediamaster-ffi` | `mediamaster-ffi` | 0.4.0 | 🆕 | Thin runtime-free FFI bridge for C interop |

## Native codec actors (scaffolded, device-validation-pending)

`mediamaster-codec` ships feature-gated native actors behind two optional features:

* `mf` → Windows Media Foundation (COM, `windows 0.62`, `Win32_Media_MediaFoundation` + `Win32_System_Threading`)
* `mediacodec` → Android MediaCodec NDK (`ndk-context` + `ndk`)

These require a Windows/Android device to validate. They are **not** published to crates.io
(they use `windows`/`ndk-sys` which only build on their target platforms). See
[docs/RELEASE.md](docs/RELEASE.md) for the coordinated 0.3.0 publish plan.

## Toolchain

```bash
cargo fmt --check        # must be clean
cargo clippy --workspace --all-targets -- -D warnings   # must be 0 errors
cargo test --workspace                              # all tests green
```

## Directory layout

```
crates/
├── mediamaster-core/     # pure Rust types + EventBus
├── mediamaster-bus/      # EventBus (tokio broadcast)
├── mediamaster-probe/    # container/head inspect + Annex-B NAL probe
├── mediamaster-codec/    # codec slotting + caps matrix
├── mediamaster-audio/    # zones, ducking, jitter buffer
├── mediamaster-agent/    # live paging + sensor ducking loop
├── mediamaster-engine/   # AudioBus, TranscodePlan, EngineSelector
├── mediamaster-rag/      # acoustic models + knowledge store
├── mediamaster-rf/       # RF demod + pipeline
├── mediamaster-sensors/  # readings, debounce, fusion, hw backends
├── mediamaster-bridge/   # provenance facade
└── mediamaster-ffi/      # thin runtime-free FFI bridge
```

## Publishing (reference)

See [docs/RELEASE.md](docs/RELEASE.md). In short:

1. `cargo check --workspace` — must be clean
2. `cargo fmt --check` — must pass
3. `cargo clippy --workspace --all-targets -- -D warnings` — must pass with 0 errors
4. `cargo test --workspace` — all tests green
5. Bump versions, commit, `cargo publish -p <crate>` (dry-run first)
6. Tag and push

## Contributing

- `cargo fmt --check` must pass with zero warnings
- All new crates get a `README.md` in their directory
- Branch → PR → merge workflow (see `.github/workflows/ci.yml`)
