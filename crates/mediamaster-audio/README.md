# mediamaster-audio

Lock-free PCM mixing, ducking, and routing.

## What it provides

- `Mixer` — fan-in an arbitrary number of `AudioSource` implementations, with a
  global `DuckingProfile` applied before writing to the output ring buffer.
- `DuckingProfile` — a ducking curve (instant / linear / exponential) that
  interpolates the output level over time.
- `MediaSourceAdapter` — reference bridge from another hardware domain (e.g.
  `tv-rs`) into `AudioSource`.
- `AudioSink` (feature `audio-cpal`) — cpal stream writer that drains the output
  ring to OS audio.
- `Zones` — a zone-based router that manages output devices, volumes, and mute states using semantic identifiers, supporting atomic sync group updates.

## Data flow

1. Sources are added to the `Mixer` as `Box<dyn AudioSource>`.
2. `Mixer::mix` reads each source, sums frames, applies ducking, and drains the
   internal `HeapRb` into a `Vec<f32>`.
3. With the `audio-cpal` feature, `AudioSink` opens the default device and
   schedules drain callback to the cpal thread.

## Integration with `tv-rs`

```rust
use mediamaster_audio::Mixer;
use mediamaster_audio::MediaSourceAdapter;

let mut mixer = Mixer::new();

// Bridge the tv-rs decoder into AudioSource.
mixer.add_source(Box::new(MediaSourceAdapter::dummy("tv-rs ch0")));

// Mix the current frame batch.
let pcm = mixer.mix();
```

## Zones

The `Zones` module provides semantic addressing for audio outputs, bridging the gap between host applications (like SignageMaster desktop) and physical playback hardware. This aligns with the companion app requirements.

- **Routing:** Assign named zones to physical output device IDs.
- **Sync Groups:** Group multiple zones (e.g. "Lobby Group") and apply volume and mute changes atomically as a single event via the `mediamaster-core` `EventBus`.
- **Typed Events:** Broadcasts `ZoneChanged` events to other crates within the ecosystem whenever volume/mute state updates.

> The `audio-cpal` feature (`AudioSink::open_default`, `start`) is wired but
> not yet exercised in tests. See the **next phase** plan in `docs/architecture.md`.
