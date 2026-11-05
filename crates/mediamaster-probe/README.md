# mediamaster-probe

Pure-Rust container & bitstream header inspector — MP4, MKV, WebM, MPEG-TS, pure audio containers (MP3, ADTS/AAC, FLAC, WAV/RF64), and still/animated images (JPEG, PNG, GIF, WebP). Zero C FFI, zero ffmpeg.

## Probe Support Matrix

| Container / Format | Magic / Framing | Key Discovered Attributes |
|---|---|---|
| **MP4 / MOV** | `ftyp` (ISO-BMFF) | Video/Audio codec, dimensions, profile, level, 10-bit, fps, duration |
| **MKV / WebM** | EBML `1A 45 DF A3` | DocType, video/audio codec, dimensions, channels, sample rate, duration |
| **MPEG-TS** | 188-byte `0x47` sync resync | PAT/PMT mapping, elementary stream types (H.264, HEVC, MPEG-2, AAC, MP3), PCR |
| **MP3** | ID3v2 skip + MPEG sync (`0xFFE`/`0xFFF`) | Layer III bitrate, sample rate, channels, estimated duration |
| **ADTS (AAC)** | `0xFFF` syncword | AAC-LC profile, sample rate, channels |
| **FLAC** | `fLaC` | STREAMINFO: sample rate, channels, bit depth, total samples, duration |
| **WAV / RF64** | `RIFF`/`RF64` ... `WAVE` | PCM/Float format, WAVEFORMATEXTENSIBLE, channels, sample rate, bit depth, ds64 64-bit size, duration |
| **JPEG** | `FF D8` (SOI) | SOF dimensions (width/height), APP1 EXIF orientation (1–8) |
| **PNG** | `89 50 4E 47...` | IHDR dimensions (width/height), `acTL` animation flag |
| **GIF** | `GIF87a` / `GIF89a` | Logical Screen Descriptor dimensions (width/height), multi-frame animation detection |
| **WebP** | `RIFF` ... `WEBP` | VP8 / VP8L / VP8X dimensions, VP8X animation flag |

## What it provides

- `MediaProbe` trait — universal sniff + parse interface.
- MP4 / ISO-BMFF box walker (encoded `d/f`, `ftyp`, `moov`, `trak`, `mdia`, `minf`, `stbl`, `stsz`, `stco`, `mp4a`, `hvc1`, etc.).
- EBML (Matroska/WebM) parser — `Segment`, `Info`, `TrackEntry`, `CodecState`, `Video`, `Audio`, `NbFrames`, etc.
- MPEG-TS 188-byte packet walker with PAT/PMT demuxing and PCR clock recovery.
- Pure audio container probing (MP3, ADTS/AAC, FLAC, WAV/RF64).
- Still and animated image metadata inspection (JPEG EXIF, PNG, GIF, WebP).
- Format sniffing — determine container + codec profile + level + image dimensions from raw bytes without spawning ffmpeg.

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
