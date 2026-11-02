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

## Native backends (feature-gated, scaffolded)

The `mf` and `mediacodec` features gate the platform-specific backing actors. Both are
staged as **portable-shaped, drop-order-law-compliant scaffolds** that compile on this
host for the portable core (`cargo check --no-default-features`); the real platform
bindings compile only on the target devices:

- **`mf` (Windows Media Foundation)** — in `src/mf_actor.rs`. A dedicated
  `CoInitializeEx` actor thread owns `MfRuntimeGuard` as its **last field**, so every
  `IMFSourceReader` / `IMFSinkWriter` / `IMFTopology` is released by the guard's
  `Drop` before `MFShutdown()` / `CoUninitialize()`. Ready to bind to the real
  `windows 0.62` `Win32_Media_MediaFoundation` COM interfaces.
- **`mediacodec` (Android MediaCodec NDK)** — in `src/ndk_codec.rs`. The
  `AndroidContext` is provided by the runtime via `ndk-context`; `JNI_OnLoad` is
  deliberately absent; `SafeNativeWindow` and the codec object are drop-ordered so a
  surface is detached before the `AMediaCodec` is torn down.

Both features are **not publishable yet**; they require device validation on Windows
(`cargo check -F mf`) and Android NDK (`cargo check -F mediacodec`) before 0.3.0.

## License

MIT OR Apache-2.0
