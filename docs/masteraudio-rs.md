# mediamaster-rs — design notes

> **Status:** draft for review · **Audience:** launcher agent + core team
> **Companion spec:** `MEDIA-SCOPE-SPEC.md` (capability matrix)
> **Goal:** a shared hardware media vocabulary that the call and phone app can
> plug into — without the workspace itself being a playback app.

---

## 1. Philosophy

Audio mixing, SDR baseband DSP, and sensor fusion are **orthogonal concerns**.
Merges them into one binary, and you pay for what you do not use (dependency
bloat, compile times, single-responsibility violation). Instead: one crate per
domain, a single shared vocabulary (`mediamaster-core`), and let each application
decide which domains to wire.

Think of `mediamaster-rs` as the **internals** of a signage player — not the
player itself. The call (desktop Tauri) and the phone app adapt the crates by
implementing the `mediamaster-core` traits; the crates never depend on the
application.

---

## 2. Crate naming strategy

Use a **shared prefix** for ecosystem discoverability while keeping names
functional.

### Recommended: `mediamaster-*` suite

| Crate | Purpose | Key dependencies |
|-------|---------|------------------|
| `mediamaster-core` | Shared event bus, traits, types | `tokio`, `serde`, `thiserror` |
| `mediamaster-audio` | Lock-free PCM mixing, ducking, routing | `mediamaster-core`, `ringbuf`; optional `cpal` (feature `audio-cpal`) |
| `mediamaster-rf` | SDR IQ pipeline, FFT detection, protocol parsing | `mediamaster-core`, `rustfft`, `nom`, `num-complex` |
| `mediamaster-sensors` | Async sensor polling, debouncing, context fusion | `mediamaster-core`, `tokio`; optional `embedded-hal-async` (feature `hw`) |
| `mediamaster-bridge` | Unified facade for external apps (`EventBus` wiring is app-owned) | `mediamaster-core`, `mediamaster-audio`, `mediamaster-rf`, `mediamaster-sensors` |

### Alternative (independent functional names)

If a prefix is not wanted: `hw-events` (event bus), `pcm-mixer` (audio), `sdr-pipeline` (SDR DSP), `sensor-hub` (sensor fusion). We use `mediamaster-*` for ecosystem cohesion.

`mediamaster-bridge` is a **new crate** (v0.1.0, `publish = false`) added in
2026-10. It holds the app-integration seam: `AudioBridge`, `SensorBridge`,
`RfBridge`, `EventHandlers`. (The three-stage RF `Pipeline` and `PipelineConfig`
live in `mediamaster-rf`, not here.)

---

## 3. Project structure

```text
mediamaster-rs/
├── Cargo.toml                          # workspace root, resolver = "2"
├── rust-toolchain.toml                 # pins Rust 1.95
├── README.md
├── CHANGELOG.md
└── crates/
    ├── mediamaster-core/               # THE FOUNDATION (traits + types + event bus)
    ├── mediamaster-audio/              # Lock-free PCM mixing + ducking
    ├── mediamaster-rf/                 # IQ acquisition → FFT → protocol parse
    ├── mediamaster-sensors/            # Sensor polling + debouncing + fusion
    ├── mediamaster-bridge/             # app-integration facade (2026-10)
    └── mediamaster-integration-tests/  # cross-crate integration tests
```

Each crate is independently publishable to crates.io. The workspace root only
defines `workspace.package` + `workspace.dependencies` and re-exports nothing.
`mediamaster-core`, `mediamaster-audio`, `mediamaster-rf`, and
`mediamaster-sensors` are published (0.1.x on crates.io); `mediamaster-bridge`
and `mediamaster-integration-tests` are `publish = false`. The next coordinated
release is **0.2.0** — see [`RELEASE.md`](RELEASE.md).

---

## 4. Integration pattern (how a caller uses this)

A caller (a Tauri desktop player, an Android phone app, or anything else) does
**not** modify this workspace. Instead it:

1. Adds the required crates as `path` dependencies (or, after publishing, as
   `crates.io` dependencies).
2. Implements the traits from `mediamaster-core`:
   - `AudioSource` — yields PCM frames into a caller-supplied buffer.
   - `AsyncSensor` — polls hardware (via `embedded-hal-async` or a stub).
   - `SignalDetector` — defines what an RF signal looks like.
3. Composes the domain crates:
   - `mediamaster-audio::Mixer` sums `AudioSource` frames + applies ducking.
   - `mediamaster-sensors::FusionEngine` aggregates `AsyncSensor` readings.
   - `mediamaster-bridge` facades (`AudioBridge`, `SensorBridge`, `RfBridge`)
     expose the domains to the app; `mediamaster-rf::Pipeline` runs the
     three-stage IQ pipeline. `EventHandlers::run(bus)` subscribes to the
     `EventBus` and routes events to the bridges.
4. The `mediamaster-core::EventBus` carries the resulting `HardwareEvent`s
   (e.g. `AudioDuckRequest`, `SensorContextChange`, `RadioBurstDetected`) between
   the domains.

> The workspace **contains no** signage app, no launcher, no device driver:
> those live in the caller's own repository.

---

## 5. What the workspace is **not**

- Not a signage player (the apps are in separate repositories).
- Not a library of hardcoded device drivers (the traits are the contract).
- Not an audio/SDR/sensor engine bundled with the sign-age app (each app
  composes the crates itself).

---

## 6. Backlog

| Phase | Work | Status |
|-------|------|--------|
| **Phase 0** | `mediamaster-rs` crate suite built + verified (core/audio/rf/sensors/bridge) | ✅ done (70 tests) |
| **Phase 0.5** | Cross-crate integration tests (workspace-level) | ✅ done |
| **Phase 1** | Publish to crates.io (`core`, `audio`, `rf`, `sensors`) | ✅ done — live at 0.1.x (2026-10-27) |
| **Phase 1.5** | Coordinated **0.2.0** release (bridge, EventHandlers, OOK demod, 3-stage pipeline, zones, ducking) | ⏸ planned — see [`RELEASE.md`](RELEASE.md) |
| **Phase 2** | Signage desktop app + phone app adapt the crates (external repos) | ⏸ pending |
| **Phase 3** | `media-scope` extraction (follow-up, Phase 2 outcome) | ⏸ pending |

> Release gate: `cargo fmt --check`, `cargo clippy --workspace
> --all-targets -- -D warnings`, `cargo test --workspace --all-features` all
> green, then one release commit per crate + tag, then `cargo publish` per
> crate (verify with `--dry-run` first).

---

## 7. Current code status (2026-11-01)

| Crate | Key API | Status |
|-------|---------|--------|
| core | `EventBus`, `HardwareEvent`, traits, `MediamasterError` | done |
| audio | `Mixer` (+ducking profile), `MediaSourceAdapter`, `Zones`, `Sink` | done |
| rf | `MagnitudeDetector`, `OokDemodulator`, `IqSource`, `ProtocolParser`, `Pipeline` (3-stage) | ✅ done |
| sensors | `DebouncedSensor`, `FusionEngine`, `SensorAggregator` | ✅ done |
| bridge | `AudioBridge`, `SensorBridge`, `RfBridge`, `EventHandlers`, device enums | ✅ done (11 unit tests + 3 doctests) |

> **State (2026-11-01):** the workspace builds clean — `cargo fmt --check`,
> `cargo clippy --workspace --all-features --all-targets -- -D warnings`, and
> `cargo test --workspace --all-features` (70 tests, 18 suites) all pass. The
> `mediamaster-rf::Pipeline` (3-stage: acquisition → DSP → OOK demod → parse)
> recovers real payloads end-to-end, `EventHandlers` routes the `EventBus` to
> the bridges, and the bridge facades are in place. Next: (a) decide when to
> lift `publish = false` and cut a crates.io release, (b) the apps (external
> repos) that implement the `mediamaster-core` traits and drive the bridges.
