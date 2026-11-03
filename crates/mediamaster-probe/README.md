# mediamaster-probe

Pure-Rust container & bitstream header inspector — MP4, MKV, WebM. Zero C FFI, zero ffmpeg.

## What it provides

- `MediaProbe` trait — universal sniff + parse interface.
- MP4 / ISO-BMFF box walker (encoded `d/f`, `ftyp`, `moov`, `trak`, `mdia`, `minf`, `stbl`, `stsz`, `stco`, `mp4a`, `hvc1`, etc.).
- EBML (Matroska/WebM) parser — `Segment`, `Info`, `TrackEntry`, `CodecState`, `Video`, `Audio`, `NbFrames`, etc.
- Format sniffing — determine container + codec profile + level from raw bytes without spawning ffmpeg.

## Why no FFI

SignageMaster devices are ARM (Android) and Windows. An ffmpeg-backed probe means linking
libavcodec, a heavy native dependency. This crate instead parses the container + bitstream
headers directly — fast, zero-dependency, and sufficient for capability detection (codec,
profile, level, chroma, bit depth, frame rate).

## Feature flags

- `std` (default) — includes `std` support.
- `alloc` — `no_std` + `alloc` builds (default feature set for publish).

## Crate graph

```text
mediamaster-probe (0.3.0)
```

## Publishing

Published to crates.io: [`mediamaster-probe`](https://crates.io/crates/mediamaster-probe) v0.3.0.
