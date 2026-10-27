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
| `mediamaster-audio` | Lock-free PCM mixing, ducking, routing | `mediamaster-core`, `cpal`, `ringbuf` |
| `mediamaster-rf` | SDR IQ pipeline, FFT detection, protocol parsing | `mediamaster-core`, `rustfft`, `nom`, `num-complex` |
| `mediamaster-sensors` | Async sensor polling, debouncing, context fusion | `mediamaster-core`, `embedded-hal-async`, `tokio` |

### Alternative (independent functional names)

If a prefix is not wanted: `hw-events` (event bus), `pcm-mixer` (audio), `sdr-pipeline` (SDR DSP), `sensor-hub` (sensor fusion). We use `mediamaster-*` for ecosystem cohesion.

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
    └── mediamaster-sensors/            # Sensor polling + debouncing + fusion
```

Each crate is independently publishable to crates.io. The workspace root only
defines `workspace.package` + `workspace.dependencies` and re-exports nothing.

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
   - `mediamaster-rf::Pipeline` (or a custom loop) emits `RadioBurstDetected`.
4. The `mediamaster-core::EventBus` carries the resulting `HardwareEvent`s
   (e.g. `AudioDuckRequest`, `SensorContextChange`, `RadioBurstDetected`)
   between the domains.

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

- [x] Phase 0 — `mediamaster-rs` crate suite built + verified (core/audio/rf/sensors).
- [x] Phase 0.5 — cross-crate integration tests (workspace-level).
- [x] Phase 1 — publish to crates.io (`core`, `audio`, `rf`, `sensors`).
- [ ] Phase 2 — sign-age desktop app + phone app adapt the crates (external repos).
- [ ] Phase 3 — `media-scope` extraction (follow-up).
