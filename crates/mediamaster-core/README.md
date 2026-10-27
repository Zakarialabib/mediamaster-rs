# mediamaster-core

Shared event bus, traits, and types for the `mediamaster-*` ecosystem.

## What it provides

- `EventBus` — `tokio::sync::broadcast` bus so `tv-rs`, `cctv-rs`, and the signage
  frontends can communicate without depending on each other.
- `AudioSource`, `AsyncSensor`, `SignalDetector` — the three adapter traits.
- `HardwareEvent`, `RoomState`, `RadioBurst`, `BoundingBox` — shared types.

## No hardware dependencies

Dependencies are limited to `tokio`, `serde`, and `thiserror`. Hardware crates
(`mediamaster-audio`, `mediamaster-rf`, `mediamaster-sensors`) depend on this
crate, never on each other.
