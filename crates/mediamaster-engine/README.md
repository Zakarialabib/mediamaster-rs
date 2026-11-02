# mediamaster-engine

Coordination layer for the `mediamaster-rs` workspace — `AudioBus`, `TranscodePlan`, `EngineSelector`.

## Overview

`mediamaster-engine` is the glue that ties the workspace's audio, codec, and
device-capability crates together:

- **`AudioBus`** — lock-free PCM ring buffer (`HeapRb` + `Mutex`) between the
  producer (agent) and the consumer (transcode / playback). SPSC ring; producer
  pushes PCM, consumer reads + applies global ducking gain.
- **`TranscodePlan`** — capability-decision: given a source resolution and the
  device's `CapsMatrix`, produce a target transcoding profile (codec, container,
  bitrate, frame rate, hardware-acceleration flag) with a human-readable reason.
- **`EngineSelector`** — per-device backend pick: `MediaFoundation`/`AndroidNdk`
  when the device advertises hardware, otherwise `Software` fallback. Portable
  `SoftwareCapsDiscovery` consults `CapsMatrix` only.

## Crate graph

```
mediamaster-engine (0.3.0)
├── mediamaster-core (0.3.0)
├── mediamaster-bus (0.3.0)
├── mediamaster-codec (0.3.0)
├── mediamaster-audio (0.3.0)
└── ringbuf (0.3)
```

## License

MIT OR Apache-2.0 (see `LICENSE`).
