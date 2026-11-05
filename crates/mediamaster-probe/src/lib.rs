//! `mediamaster-probe` — pure-Rust container & bitstream inspector.
//!
//! Probes MP4/MOV (ISO-BMFF) and Matroska/WebM byte streams and returns a
//! [`MediaDescriptor`] (container, brand, duration, video/audio codec,
//! dimensions, profile/level/bit-depth) with **zero C FFI** and **zero
//! `ffmpeg` process spawns**.
//!
//! # Example
//!
//! ```no_run
//! use mediamaster_probe::{probe_bytes, Container};
//!
//! let data = std::fs::read("clip.mp4").unwrap();
//! let desc = probe_bytes(&data).unwrap();
//! assert_eq!(desc.container, Container::Mp4);
//! if let Some(v) = &desc.video {
//!     println!("{:?} {}x{}", v.codec, v.width.unwrap_or(0), v.height.unwrap_or(0));
//! }
//! ```

pub mod audio_containers;
pub mod ebml;
pub mod error;
pub mod image;
pub mod mp4;
pub mod ts;
pub mod types;

pub use error::ProbeError;
pub use types::{
    AudioCodec, AudioInfo, Container, MediaDescriptor, StillImageInfo, VideoCodec, VideoInfo,
};

/// Byte prefix that identifies an ISO-BMFF (`ftyp`-branded) file.
const FTYP_OFFSET: usize = 4;
/// EBML / Matroska magic: `1A 45 DF A3`.
const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];
/// PNG magic: `\x89PNG\r\n\x1a\n`.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A container inspector. Implementors turn raw bytes into a
/// [`MediaDescriptor`].
pub trait MediaProbe {
    /// Probe a byte stream and describe its contents.
    fn probe_bytes(&self, data: &[u8]) -> Result<MediaDescriptor, ProbeError>;
}

/// The default container probe (dispatches across all supported formats).
#[derive(Debug, Default, Clone, Copy)]
pub struct ContainerProbe;

impl MediaProbe for ContainerProbe {
    fn probe_bytes(&self, data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
        probe_bytes(data)
    }
}

/// Probe a byte stream, sniffing the container from its magic bytes or stream sync markers.
///
/// Dispatch rules:
/// - bytes `[4..8] == b"ftyp"` → ISO-BMFF (MP4/MOV).
/// - bytes `[0..4] == 1A 45 DF A3` → Matroska / WebM.
/// - bytes `[0..2] == FF D8` → JPEG.
/// - bytes `[0..8] == 89 50 4E 47 0D 0A 1A 0A` → PNG.
/// - bytes `[0..6] == GIF87a` or `GIF89a` → GIF.
/// - bytes `[0..4] == RIFF` and `[8..12] == WEBP` → WebP.
/// - bytes `[0..4] == RIFF` / `RF64` and `[8..12] == WAVE` → WAV / RF64.
/// - bytes `[0..4] == fLaC` → FLAC.
/// - bytes `[0..3] == ID3` or MPEG frame sync → MP3.
/// - bytes `[0..2] & FFF0 == FFF0` (excluding Layer III) → ADTS (raw AAC).
/// - MPEG-TS 188-byte sync resync scan → MPEG-TS.
/// - otherwise → [`ProbeError::UnrecognizedContainer`].
pub fn probe_bytes(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }

    // 1. MP4 / MOV
    if data.len() >= FTYP_OFFSET + 4 && &data[FTYP_OFFSET..FTYP_OFFSET + 4] == b"ftyp" {
        return mp4::probe_mp4(data);
    }

    // 2. EBML (Matroska / WebM)
    if data.len() >= 4 && data[0..4] == EBML_MAGIC {
        return ebml::probe_ebml(data);
    }

    // 3. JPEG
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        return image::probe_jpeg(data);
    }

    // 4. PNG
    if data.len() >= 8 && data[0..8] == PNG_MAGIC {
        return image::probe_png(data);
    }

    // 5. GIF
    if data.len() >= 6 && (&data[0..6] == b"GIF87a" || &data[0..6] == b"GIF89a") {
        return image::probe_gif(data);
    }

    // 6. RIFF / RF64 formats (WebP, WAV, RF64)
    if data.len() >= 4 && (&data[0..4] == b"RIFF" || &data[0..4] == b"RF64") {
        if data.len() < 12 {
            return Err(ProbeError::Truncated("riff"));
        }
        if &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
            return image::probe_webp(data);
        }
        if &data[8..12] == b"WAVE" {
            return audio_containers::probe_wav(data);
        }
        return Err(ProbeError::UnrecognizedContainer);
    }

    // 7. FLAC
    if data.len() >= 4 && &data[0..4] == b"fLaC" {
        return audio_containers::probe_flac(data);
    }

    // 8. MP3 (with ID3 header)
    if data.len() >= 3 && &data[0..3] == b"ID3" {
        return audio_containers::probe_mp3(data);
    }

    // 9. ADTS (raw AAC)
    // ADTS has syncword 0xFFF (12 bits) and layer bits (bits 13..14) must be 00
    if data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xF0) == 0xF0 {
        let layer = (data[1] >> 1) & 0x03;
        if layer == 0 {
            return audio_containers::probe_adts(data);
        } else {
            // Non-zero layer with 0xFFF sync: might be MPEG Audio Layer I/II/III
            return audio_containers::probe_mp3(data);
        }
    }

    // 10. MP3 (raw MPEG audio frame without ID3, layer III has bits 13..14 != 00)
    if data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0 && (data[1] & 0x06) != 0 {
        return audio_containers::probe_mp3(data);
    }

    // 11. MPEG-TS (188-byte packet sync check)
    if ts::is_mpegts(data) {
        return ts::probe_ts(data);
    }

    Err(ProbeError::UnrecognizedContainer)
}

/// Read a file from disk and probe it.
pub fn probe_path(path: impl AsRef<std::path::Path>) -> Result<MediaDescriptor, ProbeError> {
    let data = std::fs::read(path)
        .map_err(|e| ProbeError::malformed("io", format!("failed to read file: {e}")))?;
    probe_bytes(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_errors() {
        assert_eq!(probe_bytes(&[]), Err(ProbeError::EmptyInput));
    }

    #[test]
    fn unrecognized_magic_errors() {
        assert_eq!(
            probe_bytes(&[0u8; 32]),
            Err(ProbeError::UnrecognizedContainer)
        );
    }
}
