```
 __  _____________  _______   __  ______   _____________ ______ ____ 
/  |/  / ____/ __ \/  _/   | /  |/  /   | / ___/_  __/ // / __ \ __ \
/ /|_/ / __/ / / / // // /| |/ /|_/ / /| | \__ \ / / / _  / /_/ / /_/ /
/ /  / / /___/ /_/ // // ___ / /  / / ___ |___/ // / / // / ____/ _, _/ 
/_/  /_/_____/_____/___/_/  |_/_/  /_/_/  |_/____//_/ /_//_/_/   /_/ |_| 
```

# MediaMaster (`mediamaster-rs`) — Architecture, Media Scope & Capability Specification

**Workspace:** `Zakarialabib/mediamaster-rs` (`0.2.0` Published 2026-11-01 · `0.3.0` Expansion Target)  
**Sibling Ecosystems:** [`tv-rs`](https://github.com/Zakarialabib/tv-rs) (Display/LFD/DLNA/HLS Control) · [`cctv-rs`](https://github.com/Zakarialabib/cctv-rs) (ONVIF/RTSP/PTZ/Vision Surveillance)  
**Target Hosts:** Windows Desktop (Primary Heavy Node), Linux (Edge Node), Android (Tauri Mobile Companion & Kiosk Sink)

---

## 1. Executive Summary & Ecosystem Boundary

`SignageMaster` originally embedded media probing, FFmpeg CLI process spawning, audio device control, and environmental triggers inside a monolithic `crates/media` plugin welded to internal `AppError`, SQLX, and Tauri types. 

Following the architectural discipline proven in **`tv-rs`** (epistemic capability truth, zero-`tokio` core, strict protocol isolation) and **`cctv-rs`** (hardware session budgeting, zero cleartext secrets, dual-plane control), **`mediamaster-rs`** is the standalone, open-source Rust workspace governing **in-process media intelligence, hardware-accelerated decode/encode, lock-free audio mixing/ducking, and physical room context (sensors + RF).**

### 1.1 The Three-Pillar Boundary Law (Zero Domain Overlap)

To prevent dependency bloat or split-brain hardware ownership inside `SignageMaster`, the three workspaces enforce a strict division of labor:

| Domain Concern | Authoritative Workspace | Explicit Boundary Rule |
| :--- | :--- | :--- |
| **Display Control, EDID, DLNA & Byte-Serving (`206`/HLS HTTP)** | **`tv-rs`** (`tv-rs-core`, `tv-rs-local`, `tv-rs-dlna`, `tv-rs-media`, `tv-rs-agent`) | `mediamaster-rs` **never** speaks SSDP, MDC, SSAP, or HDMI-CEC, and never hosts an `axum` HTTP server. `mediamaster-rs` produces compliant media segments/frames; `tv-rs-media` serves them. |
| **IP Cameras, ONVIF, RTSP Pull, PTZ & NALU Multiplexing** | **`cctv-rs`** (`cctv-core`, `cctv-onvif`, `cctv-rtsp`, `cctv-rtp`, `cctv-agent`) | `mediamaster-rs` **never** manages ONVIF SOAP, PTZ motors, or RTSP Digest challenges. If `mediamaster-rs` needs to transcode or record a camera feed, it consumes demuxed NALUs from `cctv-rs`. |
| **Container Probing, HW Codecs (MF/MediaCodec), Lock-Free Audio Mixing & Sensor/RF Room Fusion** | **`mediamaster-rs`** (`mediamaster-core`, `-audio`, `-sensors`, `-rf`, `-probe`, `-codec`, `-agent`) | Owns local container inspection, GPU/ASIC transcode pipelines (`windows` Media Foundation / Android `ndk` `AMediaCodec`), real-time PCM ducking (`ringbuf` + `cpal`), and physical room telemetry (`FusionEngine` + SDR `rustfft`). |

---

## 2. Workspace Architecture & Crate Inventory

Every crate lives at `crates/<dir>` and adheres to strict dependency layering. **`mediamaster-core` is strictly `tokio`-free in `0.2.0+`** so that real-time audio callback threads (`cpal`/`AAudio`), DSP pipelines (`rustfft`), and WASM/mobile consumers never link an async runtime just to share vocabulary.

```
            ┌──────────────────────────────────────────────────────────┐
            │   mediamaster-agent (Phase 4 — future coordinator)       │
            │   EngineSelector · TranscodePlan · AudioBus  [placeholder]│
            │   (CapsMatrix vocabulary itself lives in -core)          │
            └───┬──────────────┬──────────────┬──────────────┬─────────┘
                │              │              │              │
      ┌─────────▼───┐    ┌─────▼──────┐ ┌─────▼──────┐ ┌─────▼──────┐
      │   -codec    │    │   -audio   │ │  -sensors  │ │    -rf     │
      │ MF / NDK /  │    │ ringbuf /  │ │ debouncer /│ │ rustfft /  │
      │ FFmpeg-FFI  │    │ cpal / duck│ │ RoomState  │ │ nom / IQ   │
      └─────────┬───┘    └─────┬──────┘ └─────┬──────┘ └─────┬──────┘
                │              │              │              │
      ┌─────────▼──────────────▼──────────────▼──────────────▼─────────┐
      │                     mediamaster-probe                          │
      │   Pure-Rust container & bitstream inspector (MP4/MKV/WebM/TS)  │
      └────────────────────────────────┬───────────────────────────────┘
                                       ▼
      ┌────────────────────────────────────────────────────────────────┐
      │                     mediamaster-core                           │
      │  CapabilitySet, CodecCaps, MediaDescriptor, DuckingProfile,    │
      │  RoomState, RadioBurst, MediaError + Remedy.                   │
      │  ZERO TOKIO — CI gate fails if `cargo tree` shows tokio.       │
      └────────────────────────────────────────────────────────────────┘
```

### 2.1 Published Baseline (`0.1.1`) vs. `0.2.0` Expansion Matrix

| Crate | Status | Core Responsibility & Key Types |
| :--- | :--- | :--- |
| **`mediamaster-core`** | **Published (`0.2.0`)** *(0.3.0: strip `tokio`, add CapsMatrix vocab)* | **Current:** `AudioTarget`, `RoomState`, `SensorReading`, `RadioBurst`, `Signal`, `MediamasterError`, `EventBus`, and the `AudioSource`/`AsyncSensor`/`SignalDetector` traits. **To add (0.3.0):** `MediaDescriptor`, `Codec`, `PixelFormat`, `CodecCaps`, `CapsMatrix`, `MediaCapabilityBundleV1`, `MediaError` + `Remedy`; relocate `EventBus` + `AsyncSensor` to `-sensors`/`-agent` (drops `tokio`). |
| **`mediamaster-audio`** | **Published (`0.2.0`)** | Lock-free real-time PCM `Mixer` (`ringbuf::HeapRb` SPSC), smooth volume attenuation (`DuckingCurve`, `DuckingProfile`), typed audio `Zones` + `ZoneChanged` events, `cpal` hardware sink (`audio-cpal`), `symphonia` decoding (`decode`), and `MediaSourceAdapter` bridging `tv-rs` / `cctv-rs` audio triggers. |
| **`mediamaster-sensors`** | **Published (`0.2.0`)** | `DebouncedSensor`, `SensorAggregator`, and `FusionEngine` computing deterministic `RoomState` (`Empty`, `Occupied`, `LowLight`, `MotionDetected`, `PersonPresent`, `Unavailable`; `EmergencyOverride` to-add in 0.3.0) from physical GPIO/I2C/USB sensors. |
| **`mediamaster-rf`** | **Published (`0.2.0`)** | 3-stage SDR/RF `Pipeline` (acquisition → DSP → demod/parse; `AcquisitionStage` over `IqSource`), `rustfft` peak detection (`MagnitudeDetector`), `OokDemodulator` (OOK/ASK envelope demod), and `nom` frame parser (`ProtocolParser`). |
| **`mediamaster-probe`** | **New (`0.3.0` — first deliverable)** | Pure-Rust, zero-FFmpeg container & bitstream header parser (`mp4parse`, `matroska`, `symphonia-core`). Extracts exact codec profile, level, HDR10/P010 flags, chroma subsampling, and frame rate without spawning `ffmpeg`. |
| **`mediamaster-codec`** | **New (`0.4.0+` — future)** | Hardware capability discovery (`CapsMatrix`) and unified `Decoder`/`Encoder` traits over three isolated feature backends: **Windows Media Foundation (`mf`)**, **Android NDK `AMediaCodec` (`mediacodec`)**, and optional **FFmpeg FFI (`ffmpeg`)**. |
| **`mediamaster-agent`** | **New (`0.4.0+` — future)** | Autonomous reconciliation loop: watches `RoomState` + `AudioBus` + transcode queues, applies `DuckingProfile` on paging/sensor events, enforces `MaintenanceLease`, and logs health to JSONL. |

---

## 3. Epistemic Capability Truth: The `CapsMatrix` Engine

Existing Rust media crates (`ffmpeg-next`, `gstreamer`) wrap C APIs and force the caller to guess whether `-hwaccel nvenc` or `c2.qti.avc.encoder` will succeed at runtime. `mediamaster-rs` enforces **Capability Truth before execution**:

1. **Level 1 — Authoritative OS Enumeration (Zero Guessing):**
   * **Windows (`windows 0.62` `Media_Foundation` + `Dxgi`):** Calls `MFTEnumEx` with `MFT_ENUM_FLAG_HARDWARE` scoped to the active DXGI adapter LUID, reading `MFT_SUPPORT_DYNAMIC_FORMAT_CHANGE`, max resolution, and 10-bit `P010` support for NVENC/NVDEC, Intel QSV, and AMD VCN/AMF.
   * **Android (`ndk 0.9` `AMediaCodecList` / JNI):** Queries hardware codec descriptors (`c2.qti.*`, `c2.exynos.*`, `OMX.*`), explicitly filtering out software fallback decoders (`c2.android.*`) when populating `hw_accelerated: true` rows, and checking `maxSupportedInstances` to prevent Android `MediaCodec.CodecException` exhaustion.
2. **Level 2 — Quirks & SoC Seed Overlay (`quirks/default.toml`):**
   * Just as `tv-rs` pins known firmware quirks, `mediamaster-core` loads a static TOML table of known GPU/SoC driver lies (e.g., OEM Android TV boxes that advertise 4K60 HEVC encode in `MediaCodecList` but thermally throttle or crash above 1080p30).
3. **Level 3 — Synthetic Micro-Benchmark Prober (`--features bench-probe`):**
   * On first boot or idle maintenance windows, encodes a procedural 60-frame NV12 test pattern in memory across each advertised hardware encoder, measuring actual throughput (`fps_realtime_ratio`), initialization latency, and hardware slot concurrency.
   * Results are persisted as `ObservedCodecCaps` with a confidence tag (`Enumerated` vs. `VerifiedOnHardware`).

---

## 4. Desktop vs. Mobile Split (Separate Concerns by Device Capability)

Both the **Desktop App** and **Mobile App** are written in Rust + Tauri 2, sharing 100% of `mediamaster-core`'s vocabulary while compiling strictly separated dependency graphs and fulfilling complementary operational roles.

| Dimension | Desktop Node (Heavy Media Hub & Venue Controller) | Mobile App (Field Companion, Paging Source & Kiosk Sink) |
| :--- | :--- | :--- |
| **Primary Role** | Pre-ingest transcoding (MF / NVENC / QSV), multi-zone lock-free PCM mixing (`cpal`), SDR/RF + sensor fusion (`FusionEngine`), and content cache hosting. | **Role A (Field Companion):** Live voice paging/intercom injector, acoustic room calibrator, and remote `CapsMatrix` inspector.<br>**Role B (Android Signage Player):** Hardware-decoded playback (`AMediaCodec` + `AAudio`) with strict hardware instance budgeting. |
| **Crate Graph** | `mediamaster-core`, `-probe`, `-codec` (`features = ["mf"]`), `-audio` (`features = ["audio-cpal", "decode"]`), `-sensors`, `-rf`, `-agent`. | `mediamaster-core` (`+serde-sync`), `-probe`, `-codec` (`features = ["mediacodec"]`), `-audio` (mixer/ducking math only, or `AAudio` sink). **Excludes** `mf`, `ffmpeg-cli`, and heavy SDR `rf` loops. |
| **Transcode Policy** | **Transcode Producer:** Converts arbitrary user uploads (AV1/MKV/4K60) into fleet-compatible H.264/HEVC MP4/HLS profiles based on the target screens' `CapsMatrix`. | **Transcode Consumer (Never Transcodes for Fleet):** Mobile reports its `CapsMatrix` to Desktop (`MediaCapabilityBundleV1`) so Desktop pre-transcodes assets to match the Android SoC's exact hardware decode profile. |
| **Audio & Ducking** | Holds the authoritative `mediamaster-audio::Mixer` and physical amplifier outputs. | Streams low-latency Opus/PCM paging frames to the Desktop `AudioBus`, triggering an immediate `DuckingProfile` (e.g., duck background signage music to `-18 dB` over `150ms` cubic ease). |

### 4.1 Cross-Device Sync Contract (`MediaCapabilityBundleV1`)

When an Android signage player or mobile companion joins the venue fleet, it probes its local `AMediaCodec` hardware limits and `AudioManager` output topology and registers a `MediaCapabilityBundleV1` with the Desktop `mediamaster-agent`:

```rust
// In mediamaster-core (zero tokio, behind #[cfg(feature = "serde-sync")])
pub struct MediaCapabilityBundleV1 {
    pub schema_version: String,                  // "1.0"
    pub node_id: NodeId,
    pub platform: HostPlatform,                  // WindowsDesktop | LinuxEdge | AndroidMobile
    pub max_hw_decoder_instances: u8,            // Prevents Android MediaCodec slot exhaustion
    pub verified_decoders: Vec<CodecCaps>,       // H264 High@L5.1, HEVC Main10, etc.
    pub verified_encoders: Vec<CodecCaps>,
    pub audio_outputs: Vec<AudioTargetDescriptor>,
    pub probed_at_unix_ms: u64,
}
```

**Why this solves the Android Signage Crash Problem:** Low-cost Android signage SoCs (Rockchip, Amlogic, MediaTek) often support only **1 or 2 concurrent hardware 4K decoders**. If a signage layout places three 4K videos on screen simultaneously, the WebView or player silently crashes or drops to 3 fps software decoding. By reading `max_hw_decoder_instances` from `MediaCapabilityBundleV1`, the Desktop agent automatically down-mixes or pre-composites multi-zone layouts before pushing the playlist to the Android device.

---

## 5. Real-Time Thread Safety & Hardware Resource Laws

1. **The Real-Time Audio Law (`mediamaster-audio`):**
   * The `cpal` / `AAudio` render callback thread **must never** allocate heap memory (`Vec::push`, `Box::new`), lock a `std::sync::Mutex`, or call `.await`.
   * All cross-thread communication between `mediamaster-agent` (async `tokio` plane) and `mediamaster-audio` (real-time PCM plane) goes exclusively through lock-free `ringbuf::HeapRb` SPSC ring buffers and `AtomicU32` gain/ducking targets.
2. **Zero External `ffmpeg` CLI Spawning in Production:**
   * Spawning uncommitted `ffmpeg.exe` child processes is forbidden in default builds (eliminates zombie processes, broken stdout progress parsing, GPL distribution ambiguity, and Android incompatibility).
   * Windows uses in-process **Media Foundation (`IMFSourceReader` / `IMFSinkWriter`)** via `windows 0.62`; Android uses in-process **`ndk::media::AMediaCodec`**.
3. **Hardware Decoder/Encoder Slot Budgeting (`CodecSlotManager`):**
   * Mirroring `cctv-rs`'s RTSP `SessionManager`, `mediamaster-codec` tracks active hardware MFT / `AMediaCodec` instances against `max_hw_decoder_instances`. Requesting an extra hardware decoder returns a typed `MediaError::HwSlotExhausted { active, max, remedy: Remedy::FallbackToSubResolution }` instead of crashing the GPU driver.

---

## 6. Phased Execution Plan (0.2.0 shipped → 0.3.0+ expansion)

> **Alignment (2026-11-01):** (1) stripping `tokio` from core + adding CapsMatrix
> vocab are **bundled into one breaking `0.3.0`** release; (2) **`mediamaster-probe`
> is the first build deliverable**; (3) keep `mediamaster-bridge` as-is and **skip
> a separate `mediamaster` facade crate** (CapsMatrix vocabulary lives in `-core`);
> (4) expansion renumbered to `0.3.0+` — baseline is now `0.2.0` (shipped 2026-11-01).
>
> Build order ≠ phase numbering: **probe is coded first**; core purification ships
> alongside it in the same `0.3.0` release.

### Phase 1: Purify `mediamaster-core` & Add `CapsMatrix` Vocabulary — *ships in 0.3.0*
- Remove `tokio` from `mediamaster-core/Cargo.toml` (relocate `tokio::sync::broadcast` `EventBus` and `AsyncSensor` trait to `mediamaster-sensors` / `mediamaster-agent`).
- Add `Codec`, `PixelFormat`, `CodecCaps`, `CapsMatrix`, `MediaCapabilityBundleV1`, `MaintenanceLease`, `RoomState::EmergencyOverride`, and `MediaError` + `Remedy` to `mediamaster-core`.
- **Exit Gate:** `cargo tree -p mediamaster-core | grep tokio` returns empty; `cargo test -p mediamaster-core --all-features` passes.

### Phase 2: Pure-Rust Container & Bitstream Inspector (`mediamaster-probe`) — *first build deliverable*
- Create `crates/probe` (`mediamaster-probe`) using pure-Rust parsers (`mp4parse`, `symphonia-core`, EBML/MKV header reader) to inspect MP4, MKV, WebM, MP3, AAC, FLAC, and WAV files/streams without C FFI.
- Replace `SignageMaster`'s MP4-only `probe.rs` stub with `mediamaster-probe`.
- **`MediaProbe` trait + `MediaDescriptor` output shape** — [placeholder: exact fields + error mapping TBD during build].

### Phase 3: Native Hardware Codec Discovery & Engines (`mediamaster-codec`) — *0.4.0+*
- Create `crates/codec` (`mediamaster-codec`) with OS-gated native backends:
  - `#[cfg(windows)]` (`features = ["mf"]`): Enumerate hardware MFTs via `windows 0.62` (`Win32::Media::MediaFoundation`) and DXGI adapters; implement `MfDecoder` / `MfEncoder` (`IMFSinkWriter`).
  - `#[cfg(target_os = "android")]` (`features = ["mediacodec"]`): Enumerate `AMediaCodecList` via `ndk 0.9`, capture hardware decoder instance limits, and implement `NdkDecoder`.
- Implement `CodecSlotManager` to guard hardware decoder/encoder concurrency.

### Phase 4: Autonomous Audio Paging, Ducking & Sensor Reconciliation (`mediamaster-agent`) — *0.4.0+*
- Create `crates/agent` (`mediamaster-agent`) uniting `mediamaster-audio`, `mediamaster-sensors` (`FusionEngine`), and `mediamaster-rf`.
- Implement the **Live Paging & Sensor Ducking Loop**: when `FusionEngine` emits `RoomState::EmergencyOverride` (added in Phase 1) or a Mobile companion opens a live paging stream, `mediamaster-agent` commands `mediamaster-audio::Mixer` to apply the configured `DuckingProfile` and emits an audit event.