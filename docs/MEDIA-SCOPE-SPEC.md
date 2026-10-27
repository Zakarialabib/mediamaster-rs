```
 __  _____________  _______
/  |/  / ____/ __ \/  _/   |
/ /|_/ / __/ / / / // // /| |
/ /  / / /___/ /_/ // // ___ |
/_/  /_/_____/_____/_//_/_/ |_|
 _____ __________  ____  ______
/ ___// ____/ __ \/ __ \/ ____/
\__ \/ /   / / / / /_/ / __/
___/ / /___/ /_/ / ____/ /___
/____/\____/\____/_/   /_____/
```

# MediaScope — Technical Specification & Capability Matrix

**Deliverable:** Point A (SignageMaster today) → Point B (`media-scope`, an open-source Rust media-domain crate for Tauri digital signage on Windows + Android)

**Status:** DRAFT v1 · 2026-10-27 · Audience: engineering leads + product (written so non-media specialists can follow; every jargon term is defined in §12 Glossary)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Point A — What SignageMaster Can Do With Media Today](#2-point-a--what-signagemaster-can-do-with-media-today)
3. [The Gap Analysis (why we cannot "just extract another crate")](#3-the-gap-analysis)
4. [Landscape Research — Ecosystem, Engines, Crates, Benchmarks](#4-landscape-research--ecosystem-engines-crates-benchmarks)
5. [Reference Architecture Study — `tv-rs` and Friends](#5-reference-architecture-study--tv-rs-and-friends)
6. [The MediaScope Proposal — Domain Model, Traits, Facades](#6-the-mediascope-proposal--domain-model-traits-facades)
7. [Capability Matrix — Windows (`windows` crate 0.62) vs Android (`ndk` / MediaCodec / JNI)](#7-capability-matrix--windows-vs-android)
8. [Hardware & Encoding Capability Discovery (the "unknown unknowns" solver)](#8-hardware--encoding-capability-discovery)
9. [Migration Plan — Extracting From SignageMaster Without Breaking It](#9-migration-plan--extracting-from-signagemaster-without-breaking-it)
10. [Open-Source Packaging, Multi-Crate Strategy & Governance](#10-open-source-packaging-multi-crate-strategy--governance)
11. [Risks, Effort Estimates & Roadmap](#11-risks-effort-estimates--roadmap)
12. [Glossary](#12-glossary)
13. [Appendix A — Wireframes: In/Out Data Flows](#appendix-a--wireframes-inout-data-flows)
14. [Appendix B — Feature-Flag Matrix](#appendix-b--feature-flag-matrix)

---

## 1. Executive Summary

**Question asked:** *"Is our signage capable enough to extract another crate (media encode/decode)? Can we build something better than what exists — inspired by `tv-rs`, but leveraging the `windows` and `rust-mobile/ndk` crates — as a domain-level, open-source 'MediaScope' crate for our Rust+React+Tauri signage?"*

**Short answer:** **Yes, with two caveats.**

1. **Today's capability verdict (Point A):** SignageMaster already has a real media *plugin* (`crates/media`) with probing (MP4 only), thumbnails, caching, HLS/MJPEG/WebRTC streaming, camera capture, and audio-device control. But it is **not yet extractable as-is**: it is welded to internal crates (`host`, `device`, `commercial`), its own error type (`AppError`), SQLX tables, and Tauri commands. Its codec story is thin: **no decode engine, no transcode engine, no hardware-acceleration discovery** — encoding/decoding is delegated to spawning the external `ffmpeg` binary (a hard runtime dependency that does not exist on Android and is unversioned/unlicensed on desktop).
2. **The opportunity (Point B):** The Rust media ecosystem in 2026 is mature enough that a **"domain-first facade crate"** — one vocabulary (`MediaItem`, `Endpoint`, `Pipeline`, `Device`, `Permission`, `CodecCaps`) over pluggable backends (`ffmpeg`/`FFI`, `gstreamer`, Windows **Media Foundation**, Android **MediaCodec/AMedia** via `ndk`, plus pure-Rust `demuxer`/`mp4`/`symphonia`/`image`) — **does not exist yet**. Existing crates each solve one slice: `ffmpeg-next` wraps libav*, `gstreamer-rs` wraps GStreamer, `mediafoundation` wraps MF, `android_media` wraps NDK codecs, `nokhwa` wraps cameras. Nobody provides the **cross-engine capability matrix + unified trait model + permission/hardware abstraction** that a signage fleet needs. That is exactly what `tv-rs` did for TV tuners (one `Tuner` trait over OS backends), and exactly what **MediaScope** will do for *all media*.

**What we deliver here:** a full capability matrix (§7), engine/ecosystem benchmark (§4), a trait/facade/domain design (§6), a zero-risk extraction path from `crates/media` (§9), and an OSS packaging plan (§10). Estimated effort to a publishable v0.1: **~6–8 engineer-weeks**; to v1.0 parity-plus: **~20 weeks** (§11).

---

## 2. Point A — What SignageMaster Can Do With Media Today

Verified against the repository (`crates/media`, `sidecars/stream`, `src-tauri/Cargo.toml`, workspace `Cargo.lock`).

### 2.1 Inventory of the current `crates/media` plugin

| Area | Module(s) | What it actually does | Depth |
|---|---|---|---|
| **Probing / metadata** | `probe.rs` | Reads duration/width/height from **MP4 only** via `mp4parse 0.17`. Everything else returns `ProbeResult::default()` (empty). | ⚠️ Thin |
| **Thumbnails / sprites** | `thumbnail.rs`, `pipeline.rs`, `frame_cache.rs` | Image pipeline on `image 0.25` (png/jpeg/gif/webp/bmp only — AVIF gated behind `ravif` optional feature); sprite-sheet job is a **stub that logs "would invoke ffmpeg"**. | ⚠️ Partial |
| **Storage / cache** | `cache.rs`, `cache_manager.rs`, `hash.rs`, SQLX `0.8` sqlite, `cacache 13` | Content-addressable cache, sha256 dedupe, DB-backed media library, `walkdir` scanner + `notify` file watcher. | ✅ Strong |
| **Streaming out** | `hls_server.rs`, `mjpeg_gateway.rs`, `webrtc.rs`, `stream_ring.rs`, `xiu_manager.rs` | HLS server (**spawns external `ffmpeg` binary**), MJPEG gateway over axum, WebRTC signaling, Xiu live-stream manager. | ✅ Good surface, ⚠️ ffmpeg-dependent |
| **Recording** | `recording.rs` | Fragmented-MP4 recording by spawning `ffmpeg` child process. | ⚠️ ffmpeg-dependent |
| **Cameras** | `camera/*`, `capture.rs`, `vision.rs`, `zone_motion.rs` | USB camera enumeration via WMI (Windows) / nokhwa `0.10.11` (`input-native`, `decoding`); motion-zone detection hooks; AI detection mocked unless `ai-onnx-bundled`. | ✅ Decent |
| **Audio devices** | `audio.rs` | Windows-only (`cfg(windows)` + `windows 0.62` `Devices_Enumeration`/`Media_Devices`): endpoint enumeration, volume, `MediaPlayer`/`AudioGraph`/SMTC behind the `windows-media` feature. Non-Windows = graceful stubs. | ✅ Windows-only |
| **Display/GPU adjacency** | `crates/display` | Uses `windows` features `Win32_Graphics_Direct3D11`, `Dxgi`, `Win32_Media_MediaPlayer`, `Media_Miracast`; `webview2-com 0.39.1`. | Context for §7 |
| **Android media** | `crates/mobile`, `sidecars/stream` | Mobile build **excludes** `crates/media` entirely (see `src-tauri/Cargo.toml`: android-incompatible deps restricted). `ndk 0.9` appears in `Cargo.lock` **only transitively** (via `cpal`/`oboe`/tao/wry) — **we do not use `rust-mobile/ndk` directly anywhere**. Stream sidecar has `gstreamer-*` feature flags but they are **markers, not implemented** ("real impl will…" comment). | ❌ Absent |

### 2.2 Encode/decode verdict at Point A

| Question | Answer |
|---|---|
| Can we decode arbitrary video/audio in-process? | **No.** No decoder exists; playback relies on the WebView HTML5 `<video>` (OS codecs) and `ffmpeg` child processes for HLS/recording. |
| Can we encode/transcode in-process? | **No.** All transcoding = external `ffmpeg` binary spawn (`recording.rs`, `hls_server.rs`, `xiu_manager.rs`). Not available on Android; GPL/LGPL license posture must be declared on desktop distributions. |
| Can we enumerate hardware codecs (NVENC/QSV/AMF/MediaCodec/MF)? | **No.** `HwAccelConfig` exists in `pipeline.rs` as a config struct, nothing queries the device. |
| Is `crates/media` extractable as an OSS crate today? | **Not cleanly.** It imports `host::error::AppError`, `device`, `commercial`, `signage-master-plugin-protocol`, tauri command wiring, and app-specific SQLX migrations. Extraction requires an adapter seam first (§9). |
| Are `windows` and `ndk` crates already in our dependency graph? | `windows 0.62` **yes** (media/device/display/host, pinned in workspace). `ndk 0.9` **transitively only** — direct usage would be new (and is safe: `rust-mobile/ndk` is Apache-2.0/MIT and designed exactly for this). |

**Bottom line:** the *domain logic* (library, cache, scheduling-facing assets, streaming surfaces) is valuable and unique; the *engine layer* (codecs, hardware, permissions) is missing or process-spawn-based. That missing engine layer is precisely what MediaScope adds — and why extracting beats continuing to bolt modules onto `crates/media`.

---

## 3. The Gap Analysis

Why "extract another crate" is both **feasible** and **insufficient** as stated:

```
                 WHAT WE HAVE (crates/media)          WHAT MEDIA SCOPE ADDS
                ┌────────────────────────────┐       ┌────────────────────────────┐
 Domain logic   │ library·cache·scanner·HLS  │ ═══►  │ keep — becomes core of     │
 (our value)    │ MJPEG·WebRTC·assets        │       │ media-scope + signage ext. │
                ├────────────────────────────┤       ├────────────────────────────┤
 Engine layer   │ mp4parse probe (MP4 only)  │ ═══►  │ pluggable engines:         │
 (missing)      │ ffmpeg SPAWN (external)    │       │ FFmpeg-FFI · GStreamer ·   │
                │ no decoder, no encoder     │       │ MediaFoundation · Mediacod │
                ├────────────────────────────┤       ├────────────────────────────┤
 HW discovery   │ HwAccelConfig struct (dead)│ ═══►  │ CodecCaps matrix, device   │
 (missing)      │                              │       │ enumeration, bench prober  │
                ├────────────────────────────┤       ├────────────────────────────┤
 Permissions/HW │ ad-hoc (WMI, cfg windows)  │ ═══►  │ unified Permission +       │
 (scattered)    │ none on Android            │       │ DeviceModel traits (UAC,   │
                │                              │       │ MANAGE_MEDIA, READ_MEDIA,  │
                │                              │       │ DRM, secure surface…)    │
                ├────────────────────────────┤       ├────────────────────────────┤
 Portability    │ host!AppError, commercial, │ ═══►  │ zero internal deps;        │
 (blocked)      │ tauri coupling             │       │ clean Cargo features       │
                └────────────────────────────┘       └────────────────────────────┘
```

**Key insight:** the blockers are *coupling*, not code quality. A 2-week "seam refactor" (§9 Phase 0) turns `crates/media` into a portable core. After that, MediaScope is additive.

---

## 4. Landscape Research — Ecosystem, Engines, Crates, Benchmarks

*(Sources: crates.io / GitHub public data, project docs, HydrogenSDK & Bytedeck encoder benchmarks, AMD/NVIDIA/Intel SDK docs, Android CTS codec lists, Microsoft MF docs — retrieved 2026-10. Numbers marked ~ are representative ranges, re-measure before publishing.)*

### 4.1 The four media "engines" you'll hear about

An **engine** = a software stack that knows how to demux (unpackage containers like MP4/HLS), decode (turn compressed bitstreams into raw frames/samples), process (scale/filter), and encode (compress back).

| Engine | Where it lives | Platforms | License | Why signage cares |
|---|---|---|---|---|
| **FFmpeg / libav\*** | Portable C library (or spawn `ffmpeg` CLI) | Win/Linux/mac/Android/iOS | LGPL/GPL (build-dependent) | King of format coverage (≈600 codecs, every container). The default. |
| **GStreamer** | Portable C framework of "elements" in pipelines | Same, strong on Linux/Android | LGPL | Best for streaming topologies (HLS/DASH/RTP); plugin ecosystem; used by many players. |
| **Media Foundation (MF)** | Built into Windows (`IMFActivate`, `IMFMediaEngine`, `IMFSinkWriter`, `MFPlay`) | Windows 10/11 only | OS component | **Zero extra binaries, zero licensing risk**; exposes NVENC/QSV/AMF hardware MFTs automatically; low-power playback. |
| **MediaCodec / NdkMedia\*** | Built into Android (`AMediaCodec`, `AMediaMuxer`, `AMediaExtractor`, `AAudio`) | Android only | OS component | The only way to get **hardware codecs on Android** (e.g. `c2.qti.avc.encoder`, `OMX.google.vp9.decoder`); mandatory for background-safe playback; Widevine L1 DRM path. |

**Formats/containers reality check (for the non-media reader):** a *container* (MP4, MKV, WebM, TS/HLS) packages *streams*; each stream uses a *codec* (video: H.264/AVC, H.265/HEVC, AV1, VP9; audio: AAC, Opus, MP3, FLAC). Hardware acceleration means a GPU/ASIC block (NVENC/NVDEC, Intel Quick Sync, AMD VCN, Android SoC VPU) does the codec math — 5–20× faster and far lower CPU/power than software (x264/x265/libaom/SVT-AV1/opus).

### 4.2 Rust crate inventory (the candidate building blocks)

| Crate | Wraps | Platform | Maintenance signal (2026) | Verdict for MediaScope |
|---|---|---|---|---|
| `ffmpeg-next` (v8 bindings) | libav\* FFI | all (needs lib/binaries) | Active; large user base | **Backend #1** — maximal format coverage |
| `gstreamer` (+ `gstreamer-video`, `-audio`, `-app`) | GStreamer | linux/win/android | Very active (GStreamer org) | **Backend #2** — streaming pipelines |
| `mediafoundation` / `windows` crate `Media_Capture`,`Media_Transcoding`,`Graphics_Capture` | MF COM APIs | Windows | `windows` 0.62 is first-class MS-maintained; community MF wrappers smaller | **Backend #3** — the Windows-native story (`windows` crate gives us raw access; wrap selectively) |
| `ndk` + `ndk-context` + `ndk-sys` (rust-mobile) | NDK C APIs incl. `media::AMediaCodec`/`AMediaFormat` | Android | Active; Apache-2.0/MIT; already in our lockfile transitively | **Backend #4** — the Android-native story |
| `android_media` (derrebus) | higher-level MediaCodec | Android | Young | Watchlist / possible shortcut |
| `oboe` / `aaudio` (rust-mobile) | AAaudio/opensles audio I/O | Android | Active | Audio backend |
| `coreaudio-rs` / `objc2-av-foundation` | macOS/iOS audio/video | apple | Active (CoreAudio-rs org, objc2) | Future backend (parity set) |
| `demuxer` (nabijaczleweli) | pure-Rust container demux (mp4/mkv/webm/ts/ogg/flac…) | all | Solid, minimal-dep | **Pure-Rust fallback** — kills the "probe only MP4" gap |
| `symphonia` | pure-Rust audio decode (aac/mp3/flac/ogg/vorbis…) | all | Active | Audio decode fallback, thumbnails-of-sound, waveform |
| `mp4` (maciejkras) / `mp4parse` | MP4 boxes | all | Active / Gecko-maintained | Keep `mp4parse`, add `mp4` for muxing |
| `image` 0.25, `ravif`, `imgref`, `resvg`, `tiny-skia` | raster/AVIF/vector | all | Active (already ours) | Imaging backend (keep) |
| `nokhwa` 0.10 | camera capture (native/input-uvc/input-ffmpeg) | win/linux/mac | Semi-active | Camera frontend over our `CaptureSource` trait |
| `webrtc-rs` / `str0m` | WebRTC | all | Active | Streaming-out backend (we already hand-roll signaling) |
| `libmpv` bindings (`mpv`, `mpv-player`) | mpv player (uses FFmpeg internally) | win/linux/mac (+android forks) | Active | Player-engine option for kiosk surfaces |
| `wgpu`, `d3d12`, `metal` | GPU | all | Very active | Zero-copy frame transport between decode→render→encode |
| `cpal`, `rodio` | cross-platform audio playback | all | cpal semi-active | Audio output fallback |
| `v4l`, `libudev` | Linux cameras | linux | Low | Later phase |
| `tauri-plugin-*` | app shell | all | Active | Transport for commands/events |

### 4.3 Encoder performance benchmark summary (industry data, 2024–2026)

Use these as **expected relative shapes** for our own prober (§8), not absolute numbers:

| Encoder | Type | Typical throughput @1080p30 (representative) | Quality-per-bit rank | Notes |
|---|---|---|---|---|
| x264 (medium) | SW | ~2–4× realtime | good baseline | Everywhere; cheap CPU cost |
| x265 (medium) | SW | ~0.3–0.8× realtime | very good | Slow; signage rarely needs it |
| SVT-AV1 | SW | ~0.5–1.5× realtime | excellent | CPU-heavy, great future-proof |
| libaom (good) | SW | ~0.1× realtime | excellent | Too slow for live, fine for pre-ingest |
| **NVENC (NVIDIA)** | HW | **~10–30× realtime** | slightly below best SW at same bitrate | Also NVDEC decodes HEVC/AV1 fast |
| **QSV (Intel)** | HW | ~8–20× realtime | good (improved each gen) | Ubiquitous on signage mini-PCs |
| **AMF/VCN (AMD)** | HW | ~8–20× realtime | good | AV1 on RDNA2+ |
| **MediaCodec (Android SoC)** | HW | ~4–15× realtime (H.264/HEVC) | good | AV1 decode on newer SoCs; check `MediaCodecList` per device |
| **MF HW MFTs (Windows)** | HW | routes to NVENC/QSV/AMF | same as above | The Windows-native door to the HW blocks |

**Takeaway:** a signage player that pre-transcodes playlists on-device should auto-select HW encoders when present, fall back to x264/SVT-AV1 otherwise, and never assume — hence the **capability matrix prober** (§8). This is a headline innovation point vs existing crates, which expose engines but not decisions.

### 4.4 Comparison of existing "solution spaces" (what we beat)

| Solution | Scope | Cross-platform | Unified traits | HW caps discovery | Signage-ready | Weakness we exploit |
|---|---|---|---|---|---|---|
| Raw `ffmpeg-next` | codecs | ✔ (binds everywhere) | ✘ (FFmpeg-shaped API only) | ✘ (manual `-hwaccel` guessing) | ✘ | LGPL clarity, learning curve, no MF/MediaCodec native path |
| Raw `gstreamer` | codecs+pipelines | ✔ | ✘ (element/pad mental model) | partial (`gst-inspect`) | ✘ | Heavy runtime, plugin availability varies |
| `tv-rs` | TV tuners only | win/linux/mac | ✔ (single trait) | n/a | ✘ | Wrong domain (tuners), proves the pattern works |
| Commercial SDKs (Brightcove, Wowza, Videoreach) | players/streamers | partial | proprietary | some | ✔ | Closed, costly, not embeddable in our Rust/Tauri stack |
| Electron signage apps' media layers | playback | ✘ (Chromium codecs) | ✘ | ✘ | ✔ basic | No transcode, no HW control, no native capture |
| **MediaScope (this spec)** | **all media, domain-first** | **✔ Win/Android first, mac/linux next** | **✔ traits+facades** | **✔ built-in prober+matrix** | **✔** | — |

---

## 5. Reference Architecture Study — `tv-rs` and Friends

[`Zakarialabib/tv-rs`](https://github.com/Zakarialabib/tv-rs/) is the proof-of-pattern: a small Rust workspace that abstracts **TV tuner hardware across operating systems**.

### 5.1 What tv-rs does right (adopt)

```
tv-rs structure (observed):
  crates/tv-core        ← platform-agnostic types + traits + state machines
  crates/tv-windows     ← Windows.BDA/ATSC backend   (implements tv-core traits)
  crates/tv-linux       ← /dev/dvb backend
  crates/tv-macos       ← IOKit/driverkit backend
  examples/cli          ← dogfood harness
  web-ui/               ← React console talking to core over IPC
```

Patterns to copy verbatim:
1. **One narrow trait per capability family** (`Tuner`, `ChannelMap`, …) defined in a `*-core` crate with **zero OS imports**.
2. **Backend crates implement the trait** and are selected at runtime via a factory, not compile-time `cfg` soup.
3. **`State`/event machine** (idle → scanning → locked) exposed as typed enums — matches signage need for deterministic UI states.
4. **Workspace + examples + light web console** — makes the crate demonstrable (critical for OSS adoption).
5. **MIT/Apache dual license** — frictionless downstream.

### 5.2 What tv-rs lacks (fix in MediaScope)

| tv-rs gap | MediaScope answer |
|---|---|
| Single domain (tuners); no encode/decode | Full media domain: probe→demux→decode→process→encode→mux→output |
| No capability negotiation between backends | `CodecCaps` matrix + `EngineSelector` (§8) |
| Error types per backend | One `MediaScopeError` taxonomy (§6.6) |
| No permission/hardware-access model | `PermissionBroker` trait (UAC / Android runtime perms / secure display) (§7.3) |
| Async story limited | tokio-first with `async_trait`, sync escape hatches |

### 5.3 Other reference architectures worth mining

- **`rust-mobile/ndk` examples** (`native-activity`, camera/codec demos) — the canonical way to call `AMediaCodec` without JVM glue; shows `ndk-context` bootstrap pattern we'll reuse.
- **GStreamer's element/pad model** — inspiration for our `Node`/`Edge` processing graph (but hidden behind simpler facades).
- **Media Foundation `IMFMediaEngine`/`IMFSinkWriter` samples** — show the minimal COM surface we must wrap for Windows-native transcode without shipping FFmpeg.
- **mpv/libmpv embedding in Qt apps** — precedent for "player engine as library" if signage kiosk wants more than WebView playback.
- **Camera2 → MediaCodec → MediaMuxer Android pipeline** (official Android docs) — the exact chain our Android recording facade maps to.

---

## 6. The MediaScope Proposal — Domain Model, Traits, Facades

**Design north star (from your brief):** *"If we want to do a thing with media in an app, today we must search many apps/folders/APIs. Group everything: structs, facades, enums, traits… it's domain logic, not OS specifics."*

### 6.1 Layered architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│  L5  APP / SIGNAGE EXTENSION        media-scope-signage (our private)   │
│      playlist-aware asset resolver · kiosk policy · fleet telemetry     │
├─────────────────────────────────────────────────────────────────────────┤
│  L4  FACADES (the "one-stop doors")                                     │
│      MediaLibrary · Transcoder · Recorder · LiveBroadcaster ·           │
│      FrameGrabber · AudioRouter · DisplayDirector · CaptureHub          │
├─────────────────────────────────────────────────────────────────────────┤
│  L3  DOMAIN TRAITS (platform-free contracts)                            │
│      Demuxer Decoder Encoder Muxer Renderer Sink Source                  │
│      CapabilityProvider PermissionBroker DeviceCatalog                   │
├─────────────────────────────────────────────────────────────────────────┤
│  L2  ENGINE ADAPTERS (thin, replaceable)                                 │
│      engine-ffmpeg · engine-gst · engine-mf(windows) ·                  │
│      engine-mediacodec(android) · engine-purerust(demuxer/symphonia)    │
├─────────────────────────────────────────────────────────────────────────┤
│  L1  CORE TYPES + ERROR + ASYNC RUNTIME CONTRACT (media-scope-core)      │
│      MediaItem Descriptor PixelFormat Sample Rate Timebase Caps Matrix   │
├─────────────────────────────────────────────────────────────────────────┤
│  L0  OS SURFACE (never seen above L2)                                    │
│      windows 0.62 (MF, WASAPI, DXGI, WGC, UWP Devices) ·                │
│      ndk 0.9 (AMediaCodec/Muxer/Extractor, AAudio, ImageReader) ·       │
│      JNI (MediaCodecList, PackageManager perms, MediaStore) ·           │
│      objc2/CoreAudio (later) · gstreamer (later)                        │
└─────────────────────────────────────────────────────────────────────────┘
```

### 6.2 Core domain types (L1) — the shared vocabulary

```rust
// media-scope-core/src/types.rs  — NO os imports allowed here (CI-enforced)

pub struct Descriptor {          // what any MediaItem resolves to
    pub id: MediaId,             // uuid
    pub kind: MediaKind,         // Video | Audio | Image | Document | Stream | TvBroadcast
    pub container: Option<Container>,   // Mp4 | Matroska | WebM | Ts | Hls | Dash | Mjpeg...
    pub streams: Vec<StreamInfo>,       // codec, resolution, fps, bitrate, lang, hdr
    pub duration: Option<TimeBase>,
    pub location: Location,             // File(PathBuf) | Http(Uri) | Udp/Rtp | Device(..) | Memory
    pub provenance: Provenance,         // where it came from: scan | download | camera | cast | drm-vault
}

pub enum Codec { H264, Hevc, Vp9, Av1, Jpeg, Png, WebP, Avif, Aac, Opus, Mp3, Flac, PCM, DolbyEAc3 /*…*/ }
pub struct PixelFormat(pub Repr);        // NV12, I420, RGBA8, BGRA8, P010(HDR), RGB565...
pub struct Caps {                        // capability atom: codec × profile × level × fmt × limits
    pub codec: Codec, pub profiles: Vec<String>, pub max_w: u32, pub max_h: u32,
    pub max_fps: f64, pub max_bitrate_kbps: u32, pub hw: bool, pub ten_bit: bool, pub orientation: Flags,
}
pub struct CapsMatrix(BiHashMap<EngineId, Vec<Caps>>);   // THE central artifact (§8)
```

### 6.3 Trait contracts (L3) — one verb per trait

```rust
#[async_trait] pub trait Demuxer: Send + Sync {
    async fn open(&self, loc: &Location) -> Result<DemuxStream>;
    async fn read_packet(&mut self) -> Result<Option<Packet>>;      // timestamped bitstream chunk
    fn tracks(&self) -> Vec<TrackRef>;                               // selectable audio/sub/video tracks
}
#[async_trait] pub trait Decoder: Send + Sync {
    async fn configure(&mut self, track: &TrackRef, target: &DecodeTarget) -> Result<()>;
    async fn push(&mut self, pkt: Packet) -> Result<()>;
    async fn pull_frame(&mut self) -> Result<Option<Frame>>;         // Frame owns GPU/CPU buffer handle
    fn supported_caps(&self) -> &[Caps];                             // feeds CapsMatrix
}
#[async_trait] pub trait Encoder: Send + Sync {
    async fn configure(&mut self, preset: &EncodePreset) -> Result<()>; // e.g. H264/High/L4, NVENC, 8Mbps CBR
    async fn send_frame(&mut self, f: Frame) -> Result<()>;
    async fn recv_packet(&mut self) -> Result<Option<Packet>>;
    fn supported_caps(&self) -> &[Caps];
}
#[async_trait] pub trait Sink: Send + Sync {   // where finished media goes
    async fn start(&mut self) -> Result<()>;
    async fn write(&mut self, item: Renderable) -> Result<()>;
    async fn stop(&mut self) -> Result<Receipt>;                     // bytes, dropped frames, errors
}
pub trait CapabilityProvider { fn probe(&self) -> Pin<Box<dyn Future<Output = CapsMatrix>>>; }
pub trait PermissionBroker   { fn required(&self, op: &Op) -> Vec<Permission>;
                               fn status(&self, p: &Permission) -> PermState;
                               async fn request(&self, p: &Permission) -> Result<Grant>; }
pub trait DeviceCatalog      { fn outputs(&self) -> Vec<AudioOutput>; fn inputs(&self) -> Vec<CaptureInput>;
                               fn displays(&self) -> Vec<DisplaySurface>; fn vpus(&self) -> Vec<HwEngine>; }
```

**Enums-as-state (tv-rs pattern):** `PlaybackState{Idle,Preparing,Playing,Paused,Seeking,Stalled(Reason),Ended,Failed}`; `RecordState{Armed,Recording(Pct),Finalizing(File)};` `TranscodeJob{Queued,Running{fps,speed},Done{Artifact},Failed{Retry}}` — every facade exposes one of these, so the React UI binds a single reducer.

### 6.4 Facades (L4) — the "one-stop doors" (the UX innovation)

| Facade | One-line contract | Replaces the old scatter of |
|---|---|---|
| `MediaLibrary` | "give me everything playable, deduped, cached, watched" | walkdir scans + sqlx + cacache + mime_guess + notify |
| `Transcoder` | `plan(input, goal) → Job` — picks engine via CapsMatrix | hand-written ffmpeg CLI strings, guessed `-hwaccel` |
| `Recorder` | capture window/camera/screen → fragmented MP4/WebM | ffmpeg spawn + nokhwa ad-hoc |
| `LiveBroadcaster` | source → HLS+MJPEG+WebRTM endpoints w/ health | hls_server + mjpeg_gateway + webrtc + xiu_manager |
| `FrameGrabber` | poster/thumbnail/sprite at offset i | probe.rs + thumbnail.rs + frame_cache |
| `AudioRouter` | pick output device, volume, ducking, routing | audio.rs (win-only) + CPAL + Android AudioManager(JNI) |
| `DisplayDirector` | list/assign monitors·HDMI·Miracast·secure surface | crates/display windows cfg blocks |
| `CaptureHub` | cameras/mics/screen-share as uniform Sources | capture.rs + camera/* + WGC + Android CameraX(JNI)+ImageReader |
| `RightsGate` | "may I play/transcode/record this, under which license/DRM?" | (new — nothing today) |

Example — the entire signage "make this playlist safe for this screen" flow becomes:

```rust
let ms = MediaScope::builder().enable(Mf).enable(Ffmpeg).enable(MediaCodec).finish()?;
let caps = ms.caps().await?;                       // §8 matrix, cached per device
let plan = ms.transcode().plan(&item, Goal::Screen{ res: d.mode(), codec_pref: &[Av1,H264] })?;
match plan.engine() { Some(hw) => log!("using {}", hw), None => log!("SW fallback: {}", plan.sw()) }
let job = ms.transcode().run(plan).await?;         // progress events on ms.events()
ms.library().ingest(job.artifact()).await?;
```

### 6.5 Adapter selection & feature flags

Backends chosen at **runtime** by `EngineSelector` scoring (format support × HW presence × power policy × license policy), with **compile-time** Cargo features pruning what's linked:

`features = ["engine-ffmpeg", "engine-mf", "engine-mediacodec", "engine-gst", "engine-purerust", "ipc-tauri", "drm-widevine", "bench"]` — see Appendix B for the full matrix and CI story.

### 6.6 Error taxonomy

`MediaScopeError::{ Unsupported(Codec, Ctx), EngineUnavailable(EngineId, Reason), PermissionDenied(Permission, Hint), DeviceGone(DeviceId), Corrupt(Location, Offset), Policy(Blocked), Timeout(Op, Elapsed), Os(NativeCode) }` — mapped once per adapter; `Hint` carries actionable text ("Enable Hardware-accelerated GPU scheduling", "Allow ‘Do not optimize’ in battery settings").

---

## 7. Capability Matrix — Windows vs Android

> Reading guide: **Direct** = achievable with the named crate alone. **JNI/JIT-side** = needs a Java/Kotlin helper through Tauri's Android plugin bridge (NDK can't reach framework classes). **Spawns** = today's approach. ✅ shipped in SignageMaster · 🟡 exists partially · ❌ absent.

### 7.1 Decode & Playback

| Capability | Windows (`windows` 0.62) | Android (`ndk` 0.9 + JNI) | MediaScope design |
|---|---|---|---|
| SW decode any codec | `Media_Foundation` IMFSourceReader+software MFTs; or FFmpeg-FFI | `ndk::media::AMediaCodec` (sw components `c2.android.*`) ; or FFmpeg-FFI | `Decoder` trait, engine-sw path |
| HW decode H.264/HEVC/VP9/AV1 | MF auto-selects vendor MFT (NVDEC/QSV/VCN); DXGI/D3D11 surfaces | `AMediaCodec` hw components; `MediaCodecList` via JNI enumerates names | CapsMatrix `hw=true` rows |
| HDR (10-bit PQ/HLG) | MF color-space metadata, DXGI swapchain HDR | `AMediaFormat` HDR keys (API34+), HDMI info via JNI | `PixelFormat::P010`, `HdrMetadata` |
| Low-latency playback | MF `IMFMediaEngine`, WASAPI exclusive | `AAudio` (`ndk::audio`) + `AMediaCodec` async callbacks | `Sink` w/ latency budget param |
| Frame-accurate seek | MF source reader `MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING` | extractor sample-sync via JNI `MediaExtractor.seekTo` | `Demuxer::seek(TimeBase)` |
| Secure/DRM playback (Widevine) | PlayReady via MF DRM (complex) | **JNI only** (MediaDrm API); L1 attestation | `RightsGate` + `DrmAdapter` behind flag |
| WebView HTML5 playback (status quo) | wry/WebView2 `Media_Optionals` | wry/AndroidWebView (HW codecs free) | kept as `Sink::Webview` fallback |
| Status | 🟡 (WebView only; no native decode) | ❌ (media excluded from mobile build) | ✅ v0.2 |

### 7.2 Encode & Transcode

| Capability | Windows | Android | Design |
|---|---|---|---|
| H.264/HEVC HW encode | MF `IMFSinkWriter`+HW MFT (NVENC/QSV/AMF visible as `H264 Encoder MFT` etc.) | `AMediaCodec_createEncoderByName("...encoder")` + `AMediaMuxer` (`ndk::media`) | `Encoder` trait; preset table |
| AV1 HW encode | MF on RTX40/RDNA3/Arrow-Lake MFTs | newer SoCs (`c2.qti.av1.encoder`) — enumerate, don't assume | CapsMatrix row probe |
| SW encode (x264/SVT-AV1/opus) | FFmpeg-FFI engine | FFmpeg cross-compiled w/ NDK, or `mediacodec`-only + warn | engine-ffmpeg feature |
| Screen→video (signage recording/monitoring) | **WGC** `Graphics_Capture` (per-window/desktop, DXGI dupl.) | JNI `MediaProjection` (user-consent dialog) + ImageReader→encoder | `Recorder` facade |
| Camera→video | `Media_Capture` UWP `MediaCapture` init w/ `Realtime` | CameraX/JNI + `ImageReader`(AHardwareBuffer) → `AMediaCodec` | `CaptureHub`+`Encoder` |
| Thumbnail/sprite sheets | decode→scale(D3D11)→encode JPEG/AVIF (`ravif`) | decode via AMediaCodec→`RenderScript`-free scaling via GPU or sw | `FrameGrabber` |
| Audio encode (AAC/Opus) | MF `AacEncoder` MFT; FFmpeg | `AMediaCodec` `...aac.encoder`; opus via FFI | `Encoder` audio variant |
| Container mux/remux | MF sink writer (mp4); `mp4` crate pure-rust remux | `AMediaMuxer` (mp4/webm-ish) | `Muxer` trait |
| Status | ❌ (spawned ffmpeg only) | ❌ | ✅ v0.3 |

### 7.3 Devices, Permissions & Access Control (your "permissions" requirement)

| Concern | Windows mechanism | Android mechanism | Unified abstraction |
|---|---|---|---|
| Enumerate audio endpoints | `Audio_EndpointVolume`/MMDeviceEnumerator (`Win32_Media_Audio_Endpoints`) — ✅ have | JNI `AudioManager.getDevices` + `ndk::audio` AAudio | `DeviceCatalog::outputs()` |
| Set volume/mute per app | `Audio_AudioEndpoint` + `ApplicationModel_Activation` KVA | JNI AudioManager (STREAM_MUSIC) | `AudioRouter` |
| Camera enumeration | `Devices_Enumerations`/`Media_Devices` — ✅ have; WMI fallback — ✅ have | JNI `CameraManager` (lens, level) | `DeviceCatalog::inputs()` |
| Display/topology | `Win32_Devices_Display` EnumDisplaySettings — ✅ have (display crate) | JNI `DisplayManager` + HDMI connect events | `DisplayDirector` |
| Miracast casting | `Media_Miracast` ReceiveSession — ✅ have | PCCast/JNI `MediaRouter` | `DisplayDirector::cast()` |
| Microphone capture | `Media_Capture` `MediaCapture` | `RECORD_AUDIO` perm + `AudioRecord`(JNI)/AAudio | `CaptureHub` |
| **Runtime permission model** | UAC elevation (`is_elevated` — ✅ have), registry/file ACLs | `READ_MEDIA_VIDEO/AUDIO/IMAGES` (33+), `CAMERA`, `RECORD_AUDIO`, `MANAGE_MEDIA`, `MODIFY_AUDIO_SETTINGS`, `ACCESS_LOCATION`(for geo-tagged), battery-exemption for always-on | `PermissionBroker` enum→state machine with rationale strings surfaced to UI |
| Storage locations | Known folders (Videos/Pictures), removable drives | `MediaStore` (scoped storage!), SAF `content://` URIs, app-private dirs | `Location::MediaStore/Content/Drive` variants |
| Secure/protected content | PlayReady/Widevine CDM in WebView2 | Widevine L1/L3 via JNI `MediaDrm` | `RightsGate` |
| Power/kiosk constraints | `Win32_System_Power` (keepawake ✅ have), GPU scheduling policy | wakelock via JNI `PowerManager`, Doze whitelist, foreground-service type `mediaPlayback` | `PolicyHints` in plans |

### 7.4 Networking & Delivery

| Capability | Windows | Android | Design |
|---|---|---|---|
| HTTP(S) progressive + range | reqwest — ✅ have | same | `Location::Http` |
| HLS/DASH ingest & serve | ✅ have (ffmpeg-spawn hls_server) | ❌ today | `LiveBroadcaster` w/ segment-store |
| RTMP/RTSP in/out | FFmpeg/GStreamer engines | same (NDK-built FFmpeg) | `Source::Rtsp` |
| WebRTC out | webrtc-rs / str0m | same + `ACCESS_NETWORK_STATE` | `Sink::Rtc` |
| mDNS discovery (fleet) | ✅ have (mdns in net crate) | NSD via JNI | `Discovery` trait |
| Multicast/UDB | sockets | multicast-lock perm | `Source::Udp` |

### 7.5 Pure-Rust floor (works everywhere, no engines)

`demuxer`+`symphonia`+`mp4`+`image`+`ravif` give: probe **any container** (fixes §2 MP4-only gap), audio decode, image transcode, AVIF posters, MP4 remux/faststart — the **zero-dependency fallback tier** so MediaScope is useful even with no engine compiled in. This tier is what most competing crates lack.

---

## 8. Hardware & Encoding Capability Discovery

The differentiating subsystem. Three parts:

```
        ┌──────────────┐   static seed tables    ┌─────────────────┐
        │ Registry     │ (known SoCs/GPUs →      │  CapsMatrix     │◄── persisted per device-id
        │ (bundled)    │  expected codecs)       │  (BiHashMap)    │    in our sqlx cache
        └─────┬────────┘                         └───────▲─────────┘
              │ merge                                    │ measured truth wins
   ┌──────────▼───────────┐                    ┌──────────┴───────────┐
   │ Enumerator            │  query OS APIs:   │ Micro-Benchmark      │
   │ (per engine)          │ ────────────────► │ Prober (opt-in,      │
   │ MF: IMFTransform      │  MF MFT list      │  `bench` feature)    │
   │   enumerate, DXGI     │  MediaCodecList   │  2s clips → measures │
   │   adapter Desc; NDK:  │  FFmpeg hwaccels  │  actual fps/quality  │
   │   AMediaCodecList     │  gst registry     │  per candidate       │
   └───────────────────────┘                    └──────────────────────┘
```

- **Level 1 — Enumerate:** ask each compiled engine for its codec list (MF MFT enumeration; `AMediaCodecList_getCodecCount`; `avcodec_find_encoder_by_name` sweep; `gst_registry_cache`). Cheap, instant, authoritative for *presence* (not speed).
- **Level 2 — Seed:** merge bundled JSON of known chipsets (e.g. "Jasper Lake → QSV HEVC 8-bit enc/dec") to fill gaps where enumeration lies (common with OEM MFTs).
- **Level 3 — Measure (innovation):** opt-in micro-benchmarks: encode a synthetic 2-second clip through each candidate, record fps/bitrate/VMAF-lite, store in `CapsMatrix.measured`. Signage devices run this once at install/night; results become fleet intelligence ("these 12 screens all fail AV1 enc → standardize H.264").
- **Consumers:** `Transcoder::plan()` scores candidates (coverage → hw → measured speed → power policy → license policy); `diagnostics` crate exports the matrix as a human-readable report per screen — solving the *"we must search many places to know what this box can do"* pain directly.

---

## 9. Migration Plan — Extracting From SignageMaster Without Breaking It

**Phase 0 — Seam refactor (in-repo, ~2 wks, no behavior change):**
1. Introduce `crates/media/src/port.rs`: traits `Clock`, `KeyValueStore`, `EventBus`, `FsWatcher`, `HttpClient`, `ErrorMapper` replacing direct `host::AppError`/`commercial` calls (adapter inside repo keeps compiling; AppError↔MediaScopeError conversion).
2. Move pure-domain modules (`probe`, `thumbnail`, `pipeline`, `cache*`, `hash`, `stream*`, `hls_server`, `mjpeg_gateway`, `webrtc`, `recording`, `audio` types) under a `media-core` module boundary that **cannot** import `host/device/commercial` (enforce with `cargo-deny` + a CI grep test).
3. Split Tauri commands (`commands.rs`, `media_cmds*`) into `media-ipc` shim crate that stays private.

**Phase 1 — Carve `media-scope-core` + adapters (~3 wks):** publish L1/L3 types; port `probe.rs` onto `demuxer` (kills MP4-only); wrap existing ffmpeg-spawn paths behind `engine-ffmpeg-cli` (interim), then `engine-ffmpeg-ffi`.

**Phase 2 — Native engines (~4 wks):** `engine-mf` (Windows decode/encode/WGC using the `windows` features we already pin) and `engine-mediacodec` (`ndk` + thin JNI for MediaCodecList/MediaStore/permissions), wired into `crates/mobile` (first time media runs on Android!).

**Phase 3 — Facades + CapsMatrix (~3 wks)** per §6/§8; switch SignageMaster internals to facades; delete duplicated glue.

**Phase 4 — OSS graduation (~2 wks):** rename `plugin-media-engine` references, docs site, examples (`examples/cli`, react console à la tv-rs), CI matrix (windows-latest, android via `cargo-ndk`, ubuntu pure-rust tier), benchmark suite publishes §4.3-style tables per release.

**Rollback safety:** every phase keeps the old path behind a feature flag; signage regression = golden-playlist soak on 3 device classes (Win10 mini-PC, Win11+RTX, Android tablet) comparing dropped-frame/bit-exact hashes.

---

## 10. Open-Source Packaging, Multi-Crate Strategy & Governance

### 10.1 Should it be ONE crate or many? (your "multi case")

**Recommendation: workspace of few, published as a curated set** — mirrors tv-rs and the rust-mobile family:

```
media-scope/                      (workspace, MIT OR Apache-2.0)
├── crates/core        → media-scope-core        (types+traits, no deps beyond serde/tokio/async-trait/thiserror)
├── crates/engine-ffmpeg-cli / -ffi              (optional)
├── crates/engine-mf   → windows cfg only
├── crates/engine-mediacodec → ndk (+jni) android cfg only
├── crates/engine-gst  → later
├── crates/engine-pure → demuxer+symphonia+mp4+image  (the always-works tier)
├── crates/facades     → media-scope              (THE crate users add: facades+selector+caps)
├── crates/ipc-tauri   → commands/events bridge (generic, no signage types)
├── examples/player-cli, transcode-cli, caps-report, react-console
└── benches/probe-suite
```

Single-umbrella alternative (`media-scope` with features only) rejected because engine crates carry incompatible link requirements (MF COM vs NDK symbols) — cargo targets handle that better than features. Users add **one** dependency (`media-scope`) and get the rest via feature unification; advanced users depend on sub-crates directly.

### 10.2 Licensing & legal checklist
- Dual **MIT OR Apache-2.0** (ecosystem norm; matches tv-rs, rust-mobile).
- FFmpeg: ship **bindings only**; user supplies libs (avoid GPL linking ambiguity); document LGPL builds; CLI-spawn mode sidesteps linking entirely.
- HEVC/AV1 patent pools: disclaimer clause (like FFmpeg's) — signage fleets must hold pool licenses for commercial broadcast use; `RightsGate` records declarations.
- NOTICE file attributing: FFmpeg, libaom patents, Widevine terms.

### 10.3 Adoption mechanics
- Crater-friendly semver; `MSRV = 1.85`; RFC process dir; "engines are plugins" contribution guide; benchmarks published in CI artifacts (builds trust); `media-scope` keyword SEO on crates.io; launch posts (r/rust, This Week in Rust, Tauri Discord) framed as *"the missing media domain layer for Tauri"*.
- Internal dogfood rule: SignageMaster becomes the crate's first production user — the same way tv-rs's CLI/console prove it.

---

## 11. Risks, Effort Estimates & Roadmap

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| MF COM complexity (apartment threading) | M | M | isolate in dedicated STA thread; borrow patterns from `windows` samples |
| Android JNI surface creep (framework-only APIs) | H | M | keep a tiny Kotlin plugin (Tauri supports) for MediaDrm/MediaProjection/MediaCodecList |
| FFmpeg licensing perception | M | H | bindings-only policy + CLI mode default |
| Vendor MFT quirks (OEM encoders mis-advertise) | H | M | Level-3 measurement overrides registry (§8) |
| Team media-knowledge gap | M | H | glossary §12; pair with 1 external media consultant for review gates; adopt engines incrementally (pure-rust → mf/mediacodec → ffmpeg/gst) |
| Extraction breaks signage releases | M | H | Phase 0 seams + feature-flag rollback (§9) |

**Timeline (parallelizable):** v0.1 *core+purerust+caps-enumeration* (wk 1–6) → v0.2 *mf+mediacodec decode/playback* (wk 7–12) → v0.3 *encode/transcode/recorder + signage migration complete* (wk 13–18) → v0.4 *broadcaster unify + bench prober GA* (wk 19–24) → **v1.0 GA** with mac/linux engines.

---

## 12. Glossary

| Term | Plain meaning |
|---|---|
| **Demux / Mux** | Unpack / pack a container file (MP4…) into separate timed streams |
| **Codec / Bitstream** | Compression algorithm / the compressed byte sequence |
| **Frame / Sample** | One decoded image / one decoded audio block |
| **Packet** | A chunk of encoded stream data with a timestamp |
| **HW acceleration (HW acc)** | GPU/ASIC does codec math (NVENC/NVDEC, QSV, VCN, MediaCodec) |
| **MFT** | Media Foundation Transform — a Windows codec/plugin object |
| **MediaCodec** | Android's hardware/software codec API (`AMediaCodec`) |
| **WASAPI / AAudio** | Low-latency system audio APIs (Windows / Android) |
| **WGC** | Windows Graphics Capture — grab window/desktop pixels |
| **ImageReader / AHB** | Android pixel-delivery class / AHardwareBuffer zero-copy |
| **Caps** | Declared capabilities of an engine/device (formats, resolutions) |
| **HLS / DASH / RTMP / RTP** | Streaming protocols (segmented HTTP / adaptive / ingest / UDP) |
| **CDR / VMAF** | Objective video-quality metrics used by our bench prober |
| **DRM / Widevine L1** | Content protection; L1 = hardware root-of-trust on Android |
| **Scoped storage / MediaStore / SAF** | Android's modern file-access model (why naive folder scanning fails there) |
| **Facade** | Simple front-door object hiding complex subsystems |

---

## Appendix A — Wireframes: In/Out Data Flows

### A.1 End-to-end wireframe (Point A → Point B)

```
 INGEST                          CORE (media-scope)                         OUT
┌───────────────┐        ┌───────────────────────────────────┐      ┌────────────────┐
│ Sources        │        │                                   │      │ Sinks           │
│ • SMB/USB drive├──►Scan─┤► MediaLibrary (dedupe·cache·watch)│      │ • Screen/HDMI   │
│ • HTTP/CDN URL ├──►Fetch┤   ▲ CapsMatrix ◄─ EngineSelector  ├──►──┤ • Miracast cast │
│ • Camera/Mic   ├──►Cap──┤   ▲                ▲              │      │ • HLS endpoint  │
│ • RTMP/RTSP in ├──►Pull─┤   │                │              │      │ • MJPEG gateway │
│ • Cast receiver├──►Recv─┤   ▼                ▼              │      │ • WebRTC room   │
│ • TV/Broadcast ├──►Tune─┤ ┌────────┐   ┌──────────────────┐ │      │ • File/Asset lib│
└───────────────┘  (tv-rs │ │DEMUX → │ → │DECODE → PROCESS →│→ENCODE│→│• Recording vault│
                      ext) │ │PROBE   │   │(scale·fx·overlay)│  /MUX │ └───────┬────────┘
                           └────────┘   └──────────────────┘        │         │
        Permissions/HW layer:                                        │         ▼
        PermissionBroker · DeviceCatalog · RightsGate ────────────────┘   React/TS UI
                                                                            (events bus:
                                                                             state enums → reducers)
```

### A.2 Screen wireframe — "Media Cockpit" (what the operator sees)

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ MEDIA SCOPE COCKPIT                    Device: WIN11-RTX4060   Caps ✓ cached  │
├───────────────┬───────────────────────────────────────────────────────────────┤
│ LIBRARY       │  CAPABILITY MATRIX (live)                                     │
│ ▸ Videos (214)│  codec   dec-HW  enc-HW  speed(meas)  engine    notes         │
│ ▸ Images (1.2k│  H.264   ✔NVDEC  ✔NVENC  18× rt       mf,nvenc  default       │
│ ▸ Audio (88)  │  HEVC    ✔NVDEC  ✔NVENC  14× rt       mf        pool-license! │
│ ▸ Streams (6) │  AV1     ✔NVDEC  ✘       —            ff-libsvt SW fallback   │
│ [scan][import]│  VP9     ✔       ✘       —            mf-soft                 │
├───────────────┼───────────────────────────────────────────────────────────────┤
│ SELECTED ITEM │  ACTIONS                                                      │
│ promo-4k.mp4  │  [Transcode→plan: nvenc-hevc 8M] [Make posters×12]            │
│ 3840×2160 AV1 │  [Start HLS] [Record screen] [Route audio→HDMI2]              │
│ 59:41 · HDR10 │  JOB: Queued → Running 62% (9.4×rt, 0 drops) [Cancel]         │
├───────────────┴───────────────────────────────────────────────────────────────┤
│ EVENTS: ▶ playback ok · ⚠ mic permission denied (granted 2s ago) · ✔ HLS up   │
└───────────────────────────────────────────────────────────────────────────────┘
```

### A.3 Sequence — "Transcode for this screen" (facade internals)

```
UI(TS)        MediaScope facade      EngineSelector      CapsMatrix        engine-mf / mediacodec
  │  transcode(item, Goal::Screen)      │                    │                    │
  │────────────────────────────────────►│  score(candidates) │                    │
  │                                    │───────────────────►│  lookup+measured   │
  │                                    │◄───────────────────│                    │
  │◄─── Plan{engine=nvenc, preset} ────│                    │                    │
  │  confirm                            │                    │                    │
  │────────────────────────────────────►│─configure(EncodePreset)───────────────►│
  │◄── event TranscodeJob::Running{..} ─│◄─push/pull loop (frames⇄packets)──────│
  │◄── event Done{Artifact} ────────────│  mux→faststart→library.ingest          │
```

---

## Appendix B — Feature-Flag Matrix (excerpt)

| Feature | Pulls in | Compiles on | Default | Notes |
|---|---|---|---|---|
| `engine-pure` | demuxer, symphonia, mp4, image | all | ✔ | zero native deps tier |
| `engine-mf` | `windows` (Media_Foundation, Media_Capture, Graphics_Capture, DirectX, Dxgi) | Windows | ✔(win) | STA worker thread |
| `engine-mediacodec` | `ndk`, `ndk-context`, `jni` | Android | ✔(and) | + Kotlin bridge pkg for framework APIs |
| `engine-ffmpeg-cli` | tokio process | all | ✘ | interim: replaces today's hardcoded spawns |
| `engine-ffmpeg-ffi` | ffmpeg-next | all w/ libs | ✘ | bindings-only licensing |
| `engine-gst` | gstreamer | lin/win/and | ✘ | phase-2 after v1 |
| `ipc-tauri` | tauri 2 | all | ✔(app) | generic commands/events, no signage types |
| `bench` | (synthetic clips fixtures) | all | ✘ | Level-3 prober (§8) |
| `drm-widevine` | CDM loader / JNI MediaDrm | win/and | ✘ | policy-gated, enterprise only |
| `avif` | ravif | all | ✔ | posters |

---

### Final recommendation

Proceed with **Phase 0 immediately** (seams make `crates/media` extractable regardless of the OSS decision), and green-light `media-scope` as a public workspace whose first production consumer is SignageMaster itself. The capability that no off-the-shelf crate offers — and that this spec invents — is the **decision layer**: a merged, measured `CapsMatrix` + `PermissionBroker` + facade grammar that lets a signage app say *"play/transcode/record/cast this"* once, and have the right engine chosen on every Windows box and Android screen in the fleet.
