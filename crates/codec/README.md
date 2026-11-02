# mediamaster-codec

Capability-aware codec slotting for the MediaMaster ecosystem: consumes the
`mediamaster-core` [`CapsMatrix`] vocabulary to manage concurrent hardware
codec permits and walk the `DegradationTier` fallback ladder.

Part of [mediamaster-rs](https://github.com/Zakarialabib/mediamaster-rs).

## What this crate does

- **`CodecSlotManager`** — counts concurrent hardware decoder/encoder permits
  against a host's `max_hw_decoder_instances` / `max_hw_encoder_instances`
  budget (the Android MediaCodec slot-exhaustion guard, and the equivalent
  Windows MF MFT instance budget). A `0` budget is treated as *unknown* /
  unbounded rather than "no hardware".
- **`DegradationTier` ladder** — `Tier1HwSurface → Tier2HwByteBuffer →
  Tier3SharedTextureMux → Tier4PosterFallback`; `degrade_until` walks down until
  a tier is satisfiable instead of failing outright.
- **`CapsDiscovery`** — the seam platform backends (Windows Media Foundation,
  Android MediaCodec NDK) plug into to enumerate hardware into a `CapsMatrix`.
  `SoftwareCapsDiscovery` is the portable fallback that never claims unverified
  hardware.

## Usage

```rust
use mediamaster_codec::{CodecSlotManager, SlotKind, degrade_until, next_tier};
use mediamaster_core::{CapsMatrix, DegradationTier};

let mut slots = CodecSlotManager::new(&CapsMatrix {
    max_hw_decoder_instances: 2,
    ..CapsMatrix::default()
});
slots.acquire(SlotKind::Decoder)?; // ok
```

## Native backends (not yet published)

The feature-gated native backends (`mf` for Windows Media Foundation, `mediacodec`
for the Android MediaCodec NDK) land with the 0.3.0 release; this crate ships the
portable slot-budget and degradation logic first.

## License

MIT OR Apache-2.0
