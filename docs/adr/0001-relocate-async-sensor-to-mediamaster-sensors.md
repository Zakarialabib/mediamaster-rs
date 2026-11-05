# ADR 0001: Relocate AsyncSensor from mediamaster-core to mediamaster-sensors

## Status
Accepted

## Context
In `mediamaster-rs`, `mediamaster-core` is designated as an `#![no_std]` + `alloc`-only shared vocabulary crate with zero async runtime dependencies. It defines shared types (such as `RoomState`, `SensorReading`, `HardwareEvent`) and synchronous adapter traits (`AudioSource`, `SignalDetector`).

Previously, `mediamaster-core` also defined the `AsyncSensor` trait:
```rust
pub trait AsyncSensor: Send + Sync {
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>>;
}
```

This caused two structural problems:
1. `mediamaster-core` aims to remain a pure, runtime-agnostic vocabulary crate. Retaining `AsyncSensor` with `Pin<Box<dyn Future>>` leaked async mechanics into core, while other async constructs were cleanly moved to `mediamaster-bus`.
2. As async patterns evolved (e.g. boxed future vs plain `async fn in trait`), embedders experienced signature-drift pain and trait mismatches between core definitions and domain implementations.

## Decision
1. **Relocate `AsyncSensor` to `mediamaster-sensors`**:
   The primary `AsyncSensor` trait definition is now hosted in `mediamaster-sensors::sensor::AsyncSensor` (re-exported as `mediamaster_sensors::AsyncSensor`).
2. **Deprecation Shim in `mediamaster-core`**:
   To avoid immediately breaking downstream consumers, `mediamaster-core::traits::AsyncSensor` is retained as a deprecated shim for one minor release cycle (0.3 → 0.4) with `#[deprecated(since = "0.3.1", note = "AsyncSensor has moved to mediamaster-sensors; use mediamaster_sensors::AsyncSensor")]`.
3. **Bridge Alignment**:
   `mediamaster-bridge` implements `mediamaster_sensors::AsyncSensor`.

## Consequences
- **Positive:** `mediamaster-core` stays focused on pure vocabulary and data types.
- **Positive:** Domain-specific async sensor ergonomics live in the sensors crate alongside debouncing, aggregation, and hardware HAL integrations.
- **Migration:** Existing consumers importing `mediamaster_core::traits::AsyncSensor` receive a compiler deprecation warning and should migrate their imports to `mediamaster_sensors::AsyncSensor`.
