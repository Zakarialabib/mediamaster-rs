# mediamaster-audio

Lock-free PCM mixing, ducking, and routing. Zero-FFmpeg, tokio-async-native audio pipeline.

## What it provides

- `Mixer` — fan-in an arbitrary number of `AudioSource` implementations, with a global
  `DuckingProfile` applied before writing to the output ring buffer.
- `DuckingProfile` — a ducking curve (instant / linear / exponential) that interpolates
  the output level over time.
- `MediaSourceAdapter` — reference bridge from another hardware domain (e.g. `tv-rs`)
  into `AudioSource`.
- `AudioSink` (feature `audio-cpal`) — cpal stream writer that drains the output ring to
  OS audio.
- `Zones` — a zone-based router that manages output devices, volumes, and mute states
  using semantic identifiers, supporting atomic sync group updates.
- `PagingJitterBuffer` — live paging: absorbs underruns from jittery USB/stream sources
  by buffering a fixed-size ring and smoothly releasing PCM. Prevents dropouts during
  real-time paging transitions.

## Data flow

1. Sources are added to the `Mixer` as `Box<dyn AudioSource>`.
2. `Mixer::mix` reads each source, sums frames, applies ducking, and drains the internal
  `HeapRb` into a `Vec<f32>`.
3. With the `audio-cpal` feature, `AudioSink` opens the default device and schedules drain
   callback to the cpal thread.

## Feature flags

- `std` (default) — includes `std` support.
- `audio-cpal` — cpal stream writer (needs `cpal`).
- `spotify` — `spinsvr` backend for Spotify DVP playback.
- `apple` — `ava` backend for Apple Music DVP playback.
- `youtube` — `smxy` backend for YouTube DVP playback.

## Crate graph

```text
mediamaster-audio (0.3.0)
├── mediamaster-core (0.3.0)
├── mediamaster-bus (0.3.0)
├── [optional] cpal (audio-cpal)
├── [optional] spinsvr (spotify)
├── [optional] ava (apple)
└── [optional] smxy (youtube)
```

## Publishing

Published to crates.io: [`mediamaster-audio`](https://crates.io/crates/mediamaster-audio) v0.3.0.
