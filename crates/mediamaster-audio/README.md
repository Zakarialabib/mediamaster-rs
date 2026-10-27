# mediamaster-audio

Lock-free PCM mixing, ducking, and routing.

## What it provides

- `Mixer` — fan-in an arbitrary number of `AudioSource` implementations through
  per-source `ringbuf` queues, with a global ducking multiplier.
- `DuckingProfile` — a ducking curve (instant / linear / exponential) that
  interpolates the output level over time.
- `AudioSink` — a `cpal` stream writer that drains the output ring to OS audio.
- `MediaSourceAdapter` — reference bridge from another hardware domain (e.g.
  `tv-rs`) into `AudioSource`.

## Data flow

1. Sources push PCM into their own ring buffers.
2. `Mixer::mix` reads each buffer, applies per-source gain, then applies the
   global ducking multiplier, and writes to the output ring.
3. `cpal` callback drains the output ring (zero allocations, zero blocking).

## Integration with `tv-rs`

```rust
use mediamaster_audio::Mixer;

let mut mixer = Mixer::new();

// Bridge the tv-rs decoder into AudioSource.
mixer.add_source(MediaSourceAdapter::dummy("tv-rs ch0"));

// Continuously drain the output ring to the default device.
AudioSink::open_default()?.start(&mixer);
```
