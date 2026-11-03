# mediamaster-bus

The unified application event bus for the `mediamaster-*` ecosystem.

## What it provides

- `EventBus` — a `tokio::sync::broadcast` channel with async `publish` and `subscribe`.
- `HardwareEvent` — event types emitted by hardware bridges (room changes, sensor updates, RF bursts, audio duck requests).
- `RoomState` — the room model: `Playing`, `Paused`, `Biased`, `PlayingEmergency`, `PausedEmergency`, `EmergencyOverride`.

## Why a separate crate?

`EventBus` was originally in `mediamaster-core`. It was split out in 0.3.0 so that
`mediamaster-core` stays free of `tokio` (it's `#![no_std]` + `alloc`), and so that all
hardware bridges can connect to one shared bus without circular deps.

## Feature flags

- `std` (default) — includes `std` support.
- `alloc` — `no_std` + `alloc` builds (runtime only; requires `tokio` with `sync`).

## Crate graph

```text
mediamaster-bus (0.3.0)
```

## Publishing

Published to crates.io: [`mediamaster-bus`](https://crates.io/crates/mediamaster-bus) v0.3.0.
