# mediamaster-bridge

App-integration facade — the bridge between the `mediamaster-*` workspace and the
application layer (desktop + phone).

## What it provides

- `AudioBridge` — audio zones + `PagingJitterBuffer`; the app commands paging via
  `AudioBridge::enable_paging_jitter(...)` / `push_paging_pcm(...)`.
- `SensorBridge` — async sensor reads; `SensorReading` + quality + timestamp.
- `RfBridge` — RF pipeline facade; exposes detected bursts to the app.
- `events` module — `EventHandlers` that routes `EventBus` events (duck requests → mixer,
  sensor/RF results → recorded state).
- `device` module — `DeviceRegistry`, `DeviceInfo`, `AppEvent`, `AppCommand`.

## Event bus (AppEvent / AppCommand)

The bridge exposes a unified event bus for the apps:

```rust
use mediamaster_bridge::{AppEvent, AppCommand, DeviceRegistry};

// Send a command
let cmd = AppCommand::TriggerPage { room_id: "room_101".into() };
// ... bridge dispatches to AudioBridge::page(room_id)

// Subscribe to events
let event = AppEvent::SensorUpdate { sensor_id: "temp_1".into(), reading: 22.5, unit: "C".into() };
// ... app's UI updates
```

## Device registry

The registry lists all available bridges and their capabilities so the UI can build
device selection dynamically:

```rust
use mediamaster_bridge::DeviceRegistry;

let mut reg = mediamaster_bridge::device::DeviceRegistryImpl::new();
for id in ["audio", "sensors", "rf"] {
    // populate registries
}
let devices = reg.enumerate_devices(); // Vec<DeviceInfo>
```

## Data flow

1. App sends `AppCommand` → bridge dispatches to domain crate.
2. Domain crate reads hardware / does audio → `AppEvent` published.
3. App receives `AppEvent` → updates UI.

## Publishing

Published to crates.io: [`mediamaster-bridge`](https://crates.io/crates/mediamaster-bridge) v0.1.0.
