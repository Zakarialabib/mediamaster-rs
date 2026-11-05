# Migration & integration plan — mediamaster-rs → external signage apps

> **Status:** current as of 2026-11-01 · **Audience:** launcher agent + core team
> **Companion docs:** [`masteraudio-rs.md`](docs/masteraudio-rs.md) (crate design &
> backlog) · [`architecture.md`](docs/architecture.md) (crate-by-crate) ·
> [`MEDIA-SCOPE-SPEC.md`](docs/MEDIA-SCOPE-SPEC.md) (future `media-scope` spec)
> **Workspace state:** `cargo fmt --check` clean · `cargo clippy --workspace
> --all-features --all-targets -- -D warnings` clean · `cargo test --workspace
> --all-features` → **70 tests** passing.

## 1. Goal

`mediamaster-rs` is a **shared vocabulary** for hardware media systems — a
foundation that sits *alongside* `tv-rs` (broadcast) and `cctv-rs` (security
camera). The workspace does **not** contain a signage application. Instead it
provides the crates that the signage apps implement:

- `Simple-Signage desktop windows app` (Tauri desktop player) — external repository
- `companion-app mobile android app` (phone/home-companion) — external repository

Both apps implement the traits in `mediamaster-core` and wire them to the domain
crates. They interact through the `EventBus`.

## 2. What we take FROM `mediamaster-rs` (and why)

Orthogonal by design — we do **not** drag SDR, audio, or sensors into the
signage player itself:

| From crate | Re-used capability | Drop-in target |
|---|---|---|
| `mediamaster-core` | `EventBus` (broadcast), `AudioSource`/`AsyncSensor`/`SignalDetector` traits, `HardwareEvent` | the apps' adapter layer |
| `mediamaster-audio` | `Mixer` (SPSC ringbuf), `DuckingProfile`, `Zones`, `AudioSink` (cpal) | the call's audio engine |
| `mediamaster-sensors` | `DebouncedSensor`, `FusionEngine`, `AsyncSensor` | the call's scene-controller |
| `mediamaster-rf` | `SignalDetector`, `IqSource`, `OokDemodulator`, `Pipeline` | the call's RF spectrum monitor |
| `mediamaster-bridge` | `AudioBridge`, `SensorBridge`, `RfBridge`, `EventHandlers` | ready-made facade the apps can adopt |

**Design rule:** apps implement the traits; the crates do not depend on any
application.

## 3. Current state of the workspace (verified)

```text
mediamaster-rs/
├── Cargo.toml                          # workspace root, members=["crates/*"]
├── rust-toolchain.toml                 # pins Rust 1.95
├── README.md
├── CHANGELOG.md
└── crates/
    ├── mediamaster-core/               # EventBus, traits, shared types
    ├── mediamaster-audio/              # Mixer (SPSC ringbuf), ducking, zones, cpal sink
    ├── mediamaster-rf/                 # IQ pipeline, MagnitudeDetector, OOK demod, parser
    ├── mediamaster-sensors/            # DebouncedSensor, FusionEngine
    ├── mediamaster-bridge/             # app-integration facade (publish = false)
    └── mediamaster-integration-tests/  # cross-crate tests (publish = false)
```

## 4. How the external signage apps use this workspace

The two external applications adapt the crates through the shared traits.

> **There is a bridge crate.** `mediamaster-bridge` is the in-repo facade: it
> re-exports the domains behind `AudioBridge` / `SensorBridge` / `RfBridge` and
> provides `EventHandlers` to route `EventBus` events. Apps can adopt the bridge
> directly, or wire the domain crates themselves (the bridge is `publish = false`
> today — apps use it via a `path` dependency on a local checkout).

### Signage desktop app (Tauri)
- Implements `AudioSource` wrapping the `windows` audio stack (endpoint
  enumeration, per-app volume).
- Implements `AsyncSensor` wrappers for PIR/motion and light sensors.
- Owns a `Mixer` + `FusionEngine`; runs `EventHandlers::run(bus)` (or its own
  loop) to react to `AudioDuckRequest` / `SensorContextChange` events.

### Phone companion app (Android)
- Implements the same traits behind an `ndk`/JNI audio path.
- Publishes signage frames and consumes `SensorContextChange` for scene
  adaptation (e.g. low-light → dim display).

> **No backwards-compat shim:** the two apps are written fresh against the
> `mediamaster-core` + domain crates, per the user's deprecation rule.

## 5. Reuse model (tv-rs style)

`tv-rs`: `core` defines `Tuner`/traits; `windows`/`linux`/`macos` impl them;
app selects at runtime. Here: `mediamaster-core` defines `AudioSource`/
`AsyncSensor`/SignalDetector`; the call/phone apps (or the `mediamaster-bridge`
façade) implement/wire them. Only the top of the stack is application-specific.

## 6. Release status

- **Phase 1 (initial publish) — DONE:** `mediamaster-core` 0.1.1, `mediamaster-audio`
  0.1.1, `mediamaster-rf` 0.1.0, `mediamaster-sensors` 0.1.1 are live on crates.io
  (published 2026-10-27).
- **Next release — 0.2.0:** the post-0.1.x work (bridge, EventHandlers, OOK demod,
  3-stage pipeline, zones, ducking profiles) is unreleased. See
  [`docs/RELEASE.md`](docs/RELEASE.md) for the coordinated publish plan.
- **Phase 2 — pending:** the signage desktop + phone apps adapt the crates
  (external repos).
- **Phase 3 — pending:** `media-scope` extraction (follow-up, Phase 2 outcome).

## 7. Migration Notes: In-Process HLS Production (`mediamaster-engine::hls`)

> **ffmpeg CLI no longer required for HLS production.**

Previously, consumers turned raw video/audio streams into HLS by shelling out to the `ffmpeg` CLI as spawned child processes. This introduced severe operational liabilities:
- Zombie process leaks and signal handling complexity.
- GPL licensing contamination risks.
- Hard-blocking failures on Android where platform exec-permission security models forbid executing child CLI binaries.

`mediamaster-engine` now provides an in-process, pure-Rust HLS pipeline behind the `hls` feature:
- Segments are emitted directly through the `SegmentSink` trait (`SegmentSink::put_segment`).
- Engine never hosts an HTTP server or serves bytes; the embedding app connects the sink to its own byte-serving layer (e.g. `tv-rs-media`).
- Built-in pure-Rust MPEG-TS muxer (PAT, PMT, PES, PCR) and CMAF fMP4 muxer (`ftyp`, `moov`, `moof`, `mdat`).
- Deterministic PTS-based IDR segmentation, Low-Latency HLS parts (`EXT-X-PART` / `EXT-X-PRELOAD-HINT`), and automatic degradation ladder fallbacks to poster artifacts (`DegradationTier::Tier4PosterFallback`).

> The full phase table lives in [`masteraudio-rs.md`](docs/masteraudio-rs.md) §6.
