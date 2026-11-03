# mediamaster-core

Shared event bus, traits, and types for the `mediamaster-*` ecosystem.

## What it provides

- `EventBus` — `tokio::sync::broadcast` bus so `tv-rs`, `cctv-rs`, and the signage frontends can communicate without depending on each other.
- `AudioSource`, `AsyncSensor`, `SignalDetector` — the three adapter traits.
- `HardwareEvent`, `RoomState`, `RadioBurst`, `BoundingBox` — shared types.

## No hardware dependencies

Dependencies are limited to `tokio`, `serde`, and `thiserror`. Hardware crates
(`mediamaster-audio`, `mediamaster-rf`, `mediamaster-sensors`) depend on this crate,
never on each other.

## Feature flags

- `std` (default) — includes `std` support.
- `alloc` — `no_std` + `alloc` builds (default feature set for publish).

## Crate graph

```text
mediamaster-core (0.3.0)
```

## Publishing

Published to crates.io: [`mediamaster-core`](https://crates.io/crates/mediamaster-core) v0.3.0.
