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

pub mod ebml;
pub mod error;
pub mod mp4;
pub mod types;

pub use error::ProbeError;
pub use types::{AudioCodec, AudioInfo, Container, MediaDescriptor, VideoCodec, VideoInfo};

/// Byte prefix that identifies an ISO-BMFF (`ftyp`-branded) file.
const FTYP_OFFSET: usize = 4;
/// EBML / Matroska magic: `1A 45 DF A3`.
const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

/// A container inspector. Implementors turn raw bytes into a
/// [`MediaDescriptor`].
pub trait MediaProbe {
    /// Probe a byte stream and describe its contents.
    fn probe_bytes(&self, data: &[u8]) -> Result<MediaDescriptor, ProbeError>;
}

/// The default container probe (dispatches MP4 vs Matroska/WebM by magic).
#[derive(Debug, Default, Clone, Copy)]
pub struct ContainerProbe;

impl MediaProbe for ContainerProbe {
    fn probe_bytes(&self, data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
        probe_bytes(data)
    }
}

/// Probe a byte stream, sniffing the container from its magic bytes.
///
/// Dispatch rules:
/// - bytes `[4..8] == b"ftyp"` → ISO-BMFF (MP4/MOV).
/// - bytes `[0..4] == 1A 45 DF A3` → Matroska / WebM.
/// - otherwise → [`ProbeError::UnrecognizedContainer`].
pub fn probe_bytes(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    if data.len() > FTYP_OFFSET && &data[FTYP_OFFSET..FTYP_OFFSET + 4] == b"ftyp" {
        return mp4::probe_mp4(data);
    }
    if data.len() >= 4 && data[0..4] == EBML_MAGIC {
        return ebml::probe_ebml(data);
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
