# Migration & integration plan — mediamaster-rs → external signage apps

> **Status:** current as of 2026-10-27 · **Audience:** launcher agent + core team
> **Companion specs:** `masteraudio-rs.md` (crate design) · `MEDIA-SCOPE-SPEC.md` (media domain spec)
> **Workspace state:** `cargo check --workspace` green; `cargo test --workspace` → 50+ passed; `cargo fmt --check` clean; `audio-cpal` feature builds.

## 1. Goal

`mediamaster-rs` is a **shared vocabulary** for hardware media systems — a
foundation that sits *alongside* `tv-rs` (broadcast) and `cctv-rs` (security
camera). The workspace does **not** contain a signage application. Instead it
provides the crates that the signage apps implement:

- `Simple-Signage dekstop windows app` (Tauri desktop player) — external repository
- `companion-app mobile android app` (phone/home-companion) — external repository

Both apps implement the traits in `mediamaster-core` and wire them to the domain
crates. They interact through the `EventBus` (cross-process via a shared load).

## 2. What we take FROM `mediamaster-rs` (and why)

Orthogonal by design — we do **not** drag SDR, audio, or sensors into the
signage player itself:

| From crate | Re-used capability | Drop-in target |
|---|---|---|
| `mediamaster-core` | `EventBus` (broadcast), `AudioSource` trait, `HardwareEvent` | the apps' bridge/adapter layer |
| `mediamaster-audio` | `Mixer` (SPSC ringbuf), `DuckingProfile`, `AudioSink` (cpal) | the call's audio engine |
| `mediamaster-sensors` | `DebouncedSensor`, `FusionEngine`, `AsyncSensor` | the call's scene-controller |
| `mediamaster-rf` | `SignalDetector`, `IqSource`, `AcquisitionStage` | the call's RF spectrum monitor |

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
    ├── mediamaster-core/               # EventBus, AudioSource, AsyncSensor, HardwareEvent, types
    ├── mediamaster-audio/              # Mixer (SPSC ringbuf), ducking, cpal sink
    ├── mediamaster-rf/                 # IQ pipeline, MagnitudeDetector, parser
    └── mediamaster-sensors/            # DebouncedSensor, FusionEngine
```

## 4. How the external signage apps use this workspace

The two external applications adapt the crates through the shared traits.
There is no bridge crate inside this workspace — each app wires the crates
itself (via a `path` dependency on the local checkout, or by publishing the
crates to crates.io and depending on the released versions).

### Signage desktop app (Tauri)
- Implements `AudioSource` wrapping the `windows` audio stack (endpoint
  enumeration, per-app volume).
- Implements `AsyncSensor` wrappers for PIR/motion and light sensors.
- Owns a `Mixer` + `FusionEngine`; reacts to `AudioDuckRequest` /
  `SensorContextChange` events from the bus.

### Phone companion app (Android)
- Implements the same traits behind an `ndk`/JNI audio path.
- Publishes signage frames and consumes `SensorContextChange` for scene
  adaptation (e.g. low-light → dim display).

> **No backwards-compat shim:** the two apps are written fresh against the
> `mediamaster-core` + domain crates. Old ad-hoc scripts (if any) are removed
> entirely per the user's deprecation rule.

## 5. Reuse model (tv-rs style)

`tv-rs`: `core` defines `Tuner`/traits; `windows`/`linux`/`macos` impl them;
app selects at runtime. Here: `mediamaster-core` defines `AudioSource`/
`AsyncSensor`/`SignalDetector`; the call/phone apps (or a file-based façade)
implement them. Only the top of the stack is application-specific.

## 6. What the workspace is **not**

- Not a signage player (the apps are in separate repositories).
- Not a library of hardcoded device drivers (the traits are the contract).
- Not an audio/SDR/sensor engine bundled with the sign-age app (each app
  composes the crates itself).

## 7. Backlog

- [x] Phase 0 — `mediamaster-rs` crate suite built + verified (core/audio/rf/sensors).
- [x] Phase 0.5 — cross-crate integration tests (workspace-level).
- [x] Phase 1 — publish to crates.io (`core`, `audio`, `rf`, `sensors`).
- [ ] Phase 2 — sign-age desktop app + phone app adapt the crates (external repos).
- [ ] Phase 3 — RF-only feature flag, `media-scope` extraction (follow-up).
