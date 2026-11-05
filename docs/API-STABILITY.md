# mediamaster-rs Public API Surface & Stability Review

> **Date:** November 2026
> **Status:** Active Reference
> **Audience:** Ecosystem maintainers and embedders (Windows Tauri desktop app, Android companion app)

---

## 1. Overview and Stability Classifications

This document catalogs and establishes stability guarantees across all exported public API items in the `mediamaster-rs` workspace.

Stability Tiers:
- **Stable**: Long-term contract for embedders. Breaking changes are prohibited without a minor SemVer bump (while `<1.0.0`) or major bump (`>=1.0.0`). Deprecations follow a full release cycle.
- **Emerging**: Actively evolving surface or platform-specific scaffolding. Feature-gated or subject to minor signature refinements across coordinated releases.
- **Internal-only / Accidental**: Utility functions or scaffolding types not intended for external integration. These must be marked `#[doc(hidden)]` or feature-gated behind non-default configs.

---

## 2. Crate-by-Crate Surface Audit

### 2.1. `mediamaster-core` (`#![no_std] + alloc`)
Shared vocabulary crate for hardware traits and types. Must remain free of `tokio` or async runtime linkage in its library target.

- **Stable**:
  - `traits::AudioSource` (`read_pcm`, `label`)
  - `traits::AsyncSensor` (`read_sample<'a>` returning pinned boxed future)
  - `traits::SignalDetector` (`detect`, `threshold`)
  - `events::EventPublisher` (`publish`)
  - `events::HardwareEvent` (`AudioDuckRequest`, `SensorContextChange`, `RadioBurstDetected`, `CctvMotionDetected`, `ZoneChanged`)
  - `types::RoomState`, `types::RadioBurst`, `types::BoundingBox`, `types::AudioTarget`, `types::SensorReading`, `types::Signal`
  - `error::MediamasterError`
  - `caps::CapsMatrix`, `caps::CodecCaps`, `caps::Codec`, `caps::PixelFormat`, `caps::DegradationTier`
- **Emerging**:
  - `events::HardwareEvent::EnvironmentalDrift` (atmospheric harmonization telemetry)
  - `provisioning::MediaCapabilityBundleV1`, `provisioning::MaintenanceLease`, `provisioning::NodeId`, `provisioning::HostPlatform`, `provisioning::AudioTargetDescriptor`

### 2.2. `mediamaster-bus`
Concrete broadcast event bus built over `tokio::sync::broadcast`.
- **Stable**:
  - `EventBus` (`new`, `subscribe`, `publish`, `receiver_count`)
  - `EventBusSender`, `EventBusReceiver`

### 2.3. `mediamaster-probe`
Zero-FFmpeg container inspection for MP4/WebM/MKV headers.
- **Stable**:
  - `probe(reader)` → `ContainerInfo`
  - `ContainerFormat`, `CodecInfo`, `AudioCodecInfo`, `VideoCodecInfo`
  - `Error`

### 2.4. `mediamaster-codec`
Capability-aware codec slot budgeting and platform adapter scaffolding.
- **Stable**:
  - `CodecSlotManager` (`new`, `with_budget`, `acquire`, `release`, `available`, `can_acquire`, `active_count`)
  - `SlotKind` (`Decoder`, `Encoder`), `SlotExhausted`
  - `DegradationTier` transition helpers: `next_tier`, `degrade_until`
  - `discovery::probed_caps_matrix`
- **Emerging (Platform-gated)**:
  - `#[cfg(feature = "mf")] mf_actor::{spawn_actor, MfRuntimeGuard, MfRuntimeToken, MfCommand, MfConfig, MfEventHandler}`
  - `#[cfg(feature = "mediacodec")] ndk_codec::{NdkCodec, NdkContext, SafeNativeWindow, MediaCodecList, query_max_supported_instances}`

### 2.5. `mediamaster-audio`
Lock-free PCM mixing, ducking automation, and zone management.
- **Stable**:
  - `Mixer` (`new`, `add_source`, `mix`, `ducking`, `iir_gain`, `sample_rate`, `buffer_size`)
  - `ducking::DuckingProfile`, `ducking::DuckingCurve`, `ducking::AutomationCommand`, `ducking::IirSmoother`
  - `zones::Zone`, `zones::ZoneGroup`, `zones::ZoneRegistry`
  - `jitter::PagingJitterBuffer`
  - `source::MediaSourceAdapter`
  - `sink::SinkWriter` (behind `audio-cpal`)

### 2.6. `mediamaster-rf`
SDR IQ acquisition pipeline, FFT peak detection, and protocol parsing.
- **Stable**:
  - `IqSource` trait (`acquire`, `sample_rate_hz`), `IqSample`
  - `MagnitudeDetector` (`new`, `detect`, `threshold`, `peaks`)
  - `OokDemodulator` (`new`, `demodulate`, `threshold`, `samples_per_symbol`)
  - `ProtocolParser` trait (`parse_sync`), `Header`
  - `Pipeline`, `PipelineConfig`, `AcquisitionStage`
- **Internal / Preserved**:
  - `src/rtlsdr.rs` (`RtlSdrSource`): Preserved under `#[cfg(any())]` pending modernization of upstream async drivers.

### 2.7. `mediamaster-sensors`
Sensor aggregation, debouncing, and fusion context.
- **Stable**:
  - `SensorAggregator` (`new`, `push`, `readings`, `len`, `is_empty`)
  - `DebouncedSensor` (`new`, `update`, `window`)
  - `FusionEngine` (`new`, `update`, `current_state`)
  - Re-export of `AsyncSensor`
- **Emerging**:
  - `#[cfg(feature = "hw")] hw::MockSensor`, `hw::PlaceholderSensor`, `hw::I2c`

### 2.8. `mediamaster-bridge`
Unified application facade integrating audio, sensors, and RF domains.
- **Stable**:
  - `AudioBridge` (`new`, `mixer`, `set_ducking_profile`, `push_automation`, `release_automation`, `enumerate_internal_audio_devices`, `enumerate_external_audio_devices`)
  - `SensorBridge` (`new`, `fusion`, `fusion_mut`, `enumerate_internal_sensor_devices`, `enumerate_external_sensor_devices`)
  - `RfBridge` (`new`, `detector`, `enumerate_internal_rf_devices`, `enumerate_external_rf_devices`)
  - `EventHandlers` (`new`, `handle_event`, `last_room_state`, `last_burst`, `run`)
  - Re-exports of `mediamaster_core::traits` (`AsyncSensor`, `AudioSource`, `SignalDetector`) and types (`RoomState`, `SensorReading`, `Signal`)
- **App Seam (`device` module)**:
  - `DeviceInfo` (`id`, `label`, `capabilities`, `available`, `handle`)
  - `DeviceRegistry` trait (`enumerate_devices`, `is_available`)
  - `DeviceRegistryImpl` (`new`, `add`, `enumerate_devices`, `is_available`)
  - `AppEvent` (`RoomChanged`, `SensorUpdate`, `RfBurstDetected`, `AudioDuckRequest`)
  - `AppCommand` (`TriggerPage`, `EndPage`, `SetRoomContext`, `RequestSensor`)

---

## 3. Delegation vs Duplication: Bridge App Seam Audit

### 3.1. Purpose of `AppEvent` and `AppCommand`
A core architectural question addressed in this audit is whether `mediamaster-bridge::device::{AppEvent, AppCommand}` duplicates `mediamaster_core::events::HardwareEvent` or `mediamaster-bus`.

- **Audit Finding**:
  - `HardwareEvent` represents low-level, telemetry and hardware driver signals (`AudioDuckRequest`, `SensorContextChange`, `RadioBurstDetected`, `EnvironmentalDrift`, etc.) emitted into `EventBus`.
  - `AppEvent` represents high-level UI/application data models serialized across the RPC/IPC boundaries to the Windows Tauri frontend and Android companion app (e.g. formatted `SensorUpdate { sensor_id, reading, unit }` or `RfBurstDetected { frequency_hz, amplitude, timestamp }`).
  - `AppCommand` represents high-level user/intent actions directed from UI layers into the media system (`TriggerPage`, `EndPage`, `SetRoomContext`, `RequestSensor`).
- **Delegation Contract**:
  - `EventHandlers::handle_event` bridges `HardwareEvent` directly into domain crate mixers (`AutomationCommand`) and telemetry locks (`last_room_state`, `last_burst`).
  - The bridge types do **not** replace `EventBus`; rather, `DeviceRegistryImpl` aggregates `AudioBridge`, `SensorBridge`, and `RfBridge`, exposing `DeviceInfo` without duplicating the underlying audio/sensor/RF engines.
  - No redundant bus abstractions exist; delegation is clean and preserves the boundaries between low-level drivers and UI control planes.
