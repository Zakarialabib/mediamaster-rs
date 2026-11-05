# ADR 001: Frame Packet Descriptors and Ring Buffers

## Status
Accepted

## Context
Across the `mediamaster-rs` workspace, media decoders (such as Windows Media Foundation `mf_actor.rs` COM guard and Android NDK `ndk_codec.rs` SafeNativeWindow) produce sample outputs, but there was no common, canonical type for handing decoded frames across crate boundaries without copying `Vec<u8>` on every frame. Multiple consumers require frame delivery:
- HLS segmenters and packaging pipelines
- Thumbnail and poster extraction
- Ring-buffer fan-out distribution to multiple subscribers
- Cross-device sinks (such as the desktop Windows player and the Android companion kiosk)

In addition, `mediamaster-core` is `#![no_std] + alloc` and `tokio`-free, providing the common shared vocabulary for the entire ecosystem. Real-time audio threads, embedded DSP, and WASM/mobile targets link to `mediamaster-core` without depending on standard library I/O, threads, or asynchronous runtimes.

## Decision

We split the frame plumbing into two distinct layers:

### 1. `FrameRef<'a>` and `FrameFlags` in `mediamaster-core` (no_std + alloc)
- **Zero-Copy Borrowed Descriptor**: `FrameRef<'a>` wraps a borrowed slice `&'a [u8]`, PTS/DTS timestamps in microseconds, format metadata (`Codec`, `PixelFormat`), dimensions (`width`, `height`, `stride`), and bitflags (`FrameFlags`).
- **Cross-Pillar Exchange**: Because `FrameRef<'a>` contains no heap requirements other than standard primitive representations, it lives in `mediamaster-core`. It allows codec actors, embedded pipelines, and no_std surfaces to produce or consume frames without depending on `std`, OS threading, or `tokio`.
- **Serialization**: Can be optionally serialized when the `serde-sync` feature is enabled in `mediamaster-core`.

### 2. `FramePacket`, `FrameRing`, `FanOut`, and `FramePool` in `mediamaster-frame` (std)
- **Owned Counterpart (`FramePacket`)**: Wraps an `Arc<[u8]>` payload for zero-copy reference-counted fan-out across threads and tasks.
- **Bounded SPSC Ring (`FrameRing`)**: Uses `ringbuf::HeapRb` to mirror the audio mixer's lock-free SPSC buffer architecture, with configurable backpressure policies:
  - `DropNewest`: Drops the newest incoming frame when full.
  - `OverwriteOldest`: Evicts the oldest buffered frame when full.
- **Fan-Out Distribution (`FanOut`)**: Provides single-producer to $N$-subscribers fan-out with per-subscriber rings and cursors. When slow subscribers fall behind, frames are dropped and a `HardwareEvent::FrameDrop { subscriber_id, count }` event is emitted onto the event bus.
- **Memory Pooling (`FramePool`)**: Manages reusable buffers to eliminate per-frame allocations during high-framerate playback, bounding peak heap allocations to the configured pool capacity after warmup.

## Consequences

- Low-level decoders and no_std targets can pass transient decoded frames via `FrameRef` without memory allocation.
- Higher-level asynchronous pipelines, multi-threaded players, and fan-out distributors can promote `FrameRef` to `FramePacket` using `FramePool` and safely distribute frames across multiple subscriber cursors.
- `mediamaster-frame` depends only on `mediamaster-core`, `mediamaster-bus`, `ringbuf`, and `serde`, preserving clean architectural separation without coupling to codec backends, audio mixing, or the playback engine.
