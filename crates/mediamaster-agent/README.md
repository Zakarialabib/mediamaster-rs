# mediamaster-agent

Autonomous reconciliation loop uniting audio ducking, sensor context fusion, and RF
signal presence.

## What it provides

- `MediamasterAgent` — the single runtime that ties the workspace's event bus, audio
  bridge, sensor context, and RF detector together.
- **Live Paging loop** — on `RoomState::Playing` / `EmergencyOverride`, the agent drives
  `AudioBridge` to page a room and adjust `Mixer` gain.
- **Sensor Ducking loop** — on sensor readings that cross a threshold (occupancy,
  heat), the agent commands `AudioBridge` to duck the audio at a target level.
- **RF burst response** — on `RadioBurstDetected`, triggers a paging event or
  sensor-context change.

## Data flow

1. Agent subscribes to the shared `EventBus`.
2. On `RoomState::Playing` → `AudioBridge::page(room_id)`.
3. On `RoomState::EmergencyOverride` → `AudioBridge::set_ducking_profile(...)`.
4. On `SensorUpdate` → fusion engine → decides ducking target.
5. On `RadioBurstDetected` → paging / context change.

## Crate graph

```text
mediamaster-agent (0.3.0)
├── mediamaster-core (0.3.0)
├── mediamaster-bus (0.3.0)
├── mediamaster-audio (0.3.0)
├── mediamaster-sensors (0.2.0)
├── mediamaster-rf (0.2.0)
├── mediamaster-bridge (0.1.0)
├── mediamaster-rag (0.1.0)
├── mediamaster-codec (0.3.0)
└── ringbuf (0.3)
```

## Publishing

Published to crates.io: [`mediamaster-agent`](https://crates.io/crates/mediamaster-agent) v0.3.0.
