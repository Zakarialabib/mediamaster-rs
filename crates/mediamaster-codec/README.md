# mediamaster-codec

Capability-aware codec slotting for the MediaMaster ecosystem. Consumes the
`mediamaster-core` `CapsMatrix` vocabulary to manage concurrent hardware codec permits
and walk the `DegradationTier` fallback ladder.

Part of [mediamaster-rs](https://github.com/Zakarialabib/mediamaster-rs).

## What this crate does

- **`CodecSlotManager`** — counts concurrent hardware decoder/encoder permits against a
  host's `max_hw_decoder_instances` / `max_hw_encoder_instances` budget. A `0` budget is
  treated as *unknown* / unbounded rather than "no hardware".
- **`DegradationTier` ladder** — `Tier1HwSurface → Tier2HwByteBuffer →
  Tier3SharedTextureMux → Tier4PosterFallback`; `degrade_until` walks down until a tier
  is satisfiable instead of failing outright.
- **`CapsDiscovery`** — the seam platform backends (Windows Media Foundation, Android
  MediaCodec NDK) plug into to enumerate hardware into a `CapsMatrix`.
  `SoftwareCapsDiscovery` is the portable fallback that never claims unverified hardware.

## Native actors (device-validation-pending)

Two feature-gated platform backends of raw to the `CapsDiscovery` seam:

* `mf` — Windows Media Foundation (COM, `windows 0.62`, `Win32_Media_MediaFoundation` +
  `Win32_System_Threading`). Requires a Windows toolchain.
* `mediacodec` — Android MediaCodec NDK (`ndk-context` + `ndk`). Requires an Android NDK.

These are kept **local only** (not published to crates.io) because `windows` and `ndk-sys`
only build on their target platforms. Device-level validation is on the project roadmap.

## Usage

```rust
use mediamaster_codec::{CodecSlotManager, SlotKind, degrade_until, next_tier};
use mediamaster_core::{CapsMatrix, DegradationTier};

let mut slots = CodecSlotManager::new(&CapsMatrix { ... });
let tier = degrade_until(slots, SlotKind::H264);
```

## Crate graph

```text
mediamaster-codec (0.3.0)
├── mediamaster-core (0.3.0)
├── ringbuf (0.3)
└── [optional] windows (mf) / ndk-context (mediacodec)
```

## Publishing

Published to crates.io (portable only): [`mediamaster-codec`](https://crates.io/crates/mediamaster-codec) v0.3.0.
