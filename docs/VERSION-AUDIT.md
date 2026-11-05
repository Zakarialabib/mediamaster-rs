# mediamaster-rs Version Audit & Corrective Release Plan

> **Date:** November 2026
> **Status:** Audited & Corrective Plan Established
> **Audience:** Workspace maintainers, embedders (Windows Tauri desktop app, Android companion)

---

## 1. Executive Summary

The `mediamaster-rs` workspace currently exhibits version drift across crates, git tags, and crates.io releases:
- The `v0.3.0` git tag released `mediamaster-core`, `mediamaster-bus`, `mediamaster-probe`, `mediamaster-codec`, and `mediamaster-agent` at version `0.3.0`.
- Subsequently, `mediamaster-audio` and `mediamaster-engine` were published at `0.3.0`, and `mediamaster-rag` was published at `0.1.0`.
- However, `mediamaster-rf` (0.2.0), `mediamaster-sensors` (0.2.0), and `mediamaster-bridge` (0.1.0) remained un-bumped in their manifests despite breaking API changes in `mediamaster-core` (`AsyncSensor::read` → `read_sample` returning a pinned boxed future, and `HardwareEvent::EnvironmentalDrift` variant addition).
- Downstream embedders (a Windows desktop player and an Android companion app) consuming these crates remotely experience build failures when trait signatures and types change incompatibly under same or misaligned version bounds.

This document records the exact state of all workspace crates across all sources of truth and establishes a SemVer-compliant corrective bump plan.

---

## 2. Audit Matrix

The following table summarizes each crate's version across:
1. **Local Cargo.toml** (`HEAD`)
2. **README.md** matrix
3. **crates.io** actual published versions (queried via `api.crates.io`)
4. **Git Tag** `v0.3.0` state

| Crate | Cargo.toml (HEAD) | README.md | crates.io (Actuals) | Tag `v0.3.0` | Status / Drift |
|---|---|---|---|---|---|
| `mediamaster-core` | 0.3.0 | 0.3.0 | 0.3.0, 0.2.0, 0.1.1, 0.1.0 | 0.3.0 | **Desynchronized**: HEAD added `EnvironmentalDrift` variant and `AsyncSensor::read_sample`. |
| `mediamaster-bus` | 0.3.0 | 0.3.0 | 0.3.0 | 0.3.0 | Synchronized with crates.io. |
| `mediamaster-probe` | 0.3.0 | 0.3.0 | 0.3.0 | 0.3.0 | Synchronized with crates.io. |
| `mediamaster-codec` | 0.3.0 | 0.3.0 | 0.3.0 | 0.3.0 | Synchronized with crates.io. |
| `mediamaster-audio` | 0.3.0 | 0.3.0 | 0.3.0, 0.2.0, 0.1.1, 0.1.0 | 0.3.0 (in manifest) | crates.io has 0.3.0 published. |
| `mediamaster-agent` | 0.3.0 | 0.3.0 | 0.3.0 | 0.3.0 | crates.io 0.3.0 dependencies pin `audio ^0.2.0`, `rf ^0.2.0`, `sensors ^0.2.0`, `bridge ^0.1.0`. |
| `mediamaster-engine` | 0.3.0 | 0.3.0 | 0.3.0 | Not in tag | Published to crates.io at 0.3.0 after `v0.3.0` tag. |
| `mediamaster-rag` | 0.1.0 | 0.1.0 | 0.1.0 | Not in tag | Published to crates.io at 0.1.0 after `v0.3.0` tag. |
| `mediamaster-rf` | 0.2.0 | 0.2.0 | 0.2.0, 0.1.0 | 0.2.0 | **Lagging**: Unreleased changes (pipeline refactors, bus integration); manifest left at 0.2.0. |
| `mediamaster-sensors` | 0.2.0 | 0.2.0 | 0.2.0, 0.1.1, 0.1.0 | 0.2.0 | **Lagging & Breaking**: Uses new `AsyncSensor::read_sample` from core 0.3.0; manifest left at 0.2.0. |
| `mediamaster-bridge` | 0.1.0 | 0.1.0 | 0.1.0 | 0.1.0 | **Lagging & Breaking**: Added `device` module (`DeviceInfo`, `DeviceRegistry`, `AppEvent`, `AppCommand`); implemented `read_sample`. Manifest at 0.1.0. |

---

## 3. SemVer & Public Trait Signature Analysis

### 3.1. `AsyncSensor` Trait Signature Change
In `mediamaster-core` 0.2.0:
```rust
pub trait AsyncSensor: Send + Sync {
    async fn read(&self) -> SensorReading;
}
```
In `mediamaster-core` 0.3.0 / HEAD:
```rust
pub trait AsyncSensor: Send + Sync {
    fn read_sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SensorReading, MediamasterError>> + Send + 'a>>;
}
```
- **Impact:** Any crate implementing or calling `AsyncSensor` broke when moving from 0.2.0 to 0.3.0.
- `mediamaster-sensors` and `mediamaster-bridge` updated their implementations of `AsyncSensor` to `read_sample`, but their package versions were not bumped accordingly.

### 3.2. `HardwareEvent` Enum Variant Addition
In `mediamaster-core` HEAD:
```rust
pub enum HardwareEvent {
    // ... existing variants
    EnvironmentalDrift {
        motion_density: f32,
        mmwave_distance_m: f32,
        ambient_lux: f32,
        lux_rate_of_change: f32,
        rf_burst_frequency: f32,
    },
}
```
- `HardwareEvent` is not marked `#[non_exhaustive]`. Adding a variant breaks exhaustive pattern matching in all downstream consumers. Under SemVer for `<1.0.0` crates, any breaking change requires at least a minor version bump (e.g. `0.3.0` → `0.4.0`).

### 3.3. `mediamaster-bridge` Additions
`mediamaster-bridge` introduced the `device` module exposing:
- `DeviceInfo`
- `AppEvent`
- `AppCommand`
- `DeviceRegistry` & `DeviceRegistryImpl`
- Re-exports of updated `AsyncSensor` trait

Under SemVer, for `0.1.0` crates, breaking trait updates or major public additions warrant bumping to `0.2.0`.

---

## 4. Corrective Bump Plan

To avoid breaking external embedders while allowing other development agents to progress without lockouts, we implement a **staged corrective bump plan**:

### 4.1. Immediate Version Bumps in Repository
1. **`mediamaster-sensors`**: Bump version `0.2.0` → `0.3.0`.
   - Update its `Cargo.toml` version to `0.3.0`.
   - Update `mediamaster-agent`, `mediamaster-bridge`, `mediamaster-engine`, and `mediamaster-integration-tests` dependency requirements to `mediamaster-sensors = 0.3.0`.
2. **`mediamaster-rf`**: Bump version `0.2.0` → `0.3.0`.
   - Update its `Cargo.toml` version to `0.3.0`.
   - Update workspace dependents (`agent`, `bridge`, `engine`, `integration-tests`) to `mediamaster-rf = 0.3.0`.
3. **`mediamaster-bridge`**: Bump version `0.1.0` → `0.2.0`.
   - Update its `Cargo.toml` version to `0.2.0`.
   - Update `mediamaster-agent` dependency requirement to `mediamaster-bridge = 0.2.0`.
4. Update `README.md` crate matrix to reflect the bumped versions.

### 4.2. Deferred Release Schedule (Publishing Gate)
Publishing these crates to `crates.io` is explicitly **deferred** until concurrent feature additions across active agents stabilize:
- **Phase A (Current):** Commit version bumps in `Cargo.toml` and documentation; enforce local reproducibility and CI checks.
- **Phase B (Deferred publish):** Once all concurrent agent branches merge into `main`:
  1. `mediamaster-core` release (either `0.3.1` patch or `0.4.0` minor bump for `EnvironmentalDrift`).
  2. `mediamaster-bus` release.
  3. `mediamaster-rf` (0.3.0) and `mediamaster-sensors` (0.3.0) published to crates.io.
  4. `mediamaster-bridge` (0.2.0) published to crates.io.
  5. `mediamaster-engine`, `mediamaster-rag`, `mediamaster-agent` updated and published.
  6. Git tag `v0.4.0` (or per-crate tags) cut to mark the synchronized baseline.
