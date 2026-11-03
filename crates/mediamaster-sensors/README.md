# mediamaster-sensors

Sensor reading, debounce, fusion context, and hardware backends.

## What it provides

- `AsyncSensor` trait — the async sensor adapter.
- `SensorReading` — sensor value + quality + timestamp + confidence.
- `SensorAggregator` — bounded FIFO ring of recent readings, computes averages/medians.
- `SensorFilter` — duplicate detection + rate-limiting window.
- `Debounce` — single-timestamp debounce (press-release semantics).
- `DebouncePulse` — press-release debounce with explicit pulse output.
- `fusion` module — `SensorContext` (24-class 3D pose support), `FusionEngine` (tasker),
  and the fusion pipeline.
- `hw` feature — real hardware backends via `embedded-hal-async`:
  - `Bme280Sensor` — temperature / pressure / humidity (I2C).
  - `Bno080Sensor` — 3D pose / quaternion (I2C).
  - `Mlx90640Sensor` — long-range thermal (I2C).
  - `EspNowReceiver` — ESP-NOW UDP broadcast receiver (UDP sockets).

## Data flow

1. Hardware sensor reads → `AsyncSensor` → `SensorReading`.
2. `SensorAggregator` maintains a bounded window of recent readings (averages/medians).
3. `SensorFilter` debounces / rate-limits.
4. `Debounce`/`DebouncePulse` for press-release / threshold detection.
5. `fusion::SensorContext` fuses multi-sensor streams into a unified context for the
   agent / app.

## Feature flags

- `hw` — real hardware backends (default off; requires `embedded-hal-async`).

## Crate graph

```text
mediamaster-sensors (0.2.0)
├── mediamaster-core (0.3.0)
└── [optional] embedded-hal-async (hw feature)
```

## Publishing

Published to crates.io: [`mediamaster-sensors`](https://crates.io/crates/mediamaster-sensors) v0.2.0.
