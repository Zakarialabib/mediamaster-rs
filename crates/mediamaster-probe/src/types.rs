//! Output descriptor types returned by the probe.

/// Container / bitstream container format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// ISO-BMFF (`ftyp`-branded MP4, MOV, M4V…).
    Mp4,
    /// Matroska (`matroska` DocType).
    Mkv,
    /// WebM (`webm` DocType).
    Webm,
    /// MPEG-TS (MPEG-2 Transport Stream).
    Ts,
    /// MP3 (MPEG audio Layer III).
    Mp3,
    /// Raw AAC with ADTS framing.
    Aac,
    /// Free Lossless Audio Codec (FLAC).
    Flac,
    /// WAV / RF64 PCM/audio container.
    Wav,
    /// JPEG still image.
    Jpeg,
    /// PNG still image.
    Png,
    /// GIF still or animated image.
    Gif,
    /// WebP still or animated image.
    Webp,
    /// Raw elementary stream with Annex-B NAL unit framing (H.264 / H.265).
    AnnexB,
}

impl Container {
    pub fn as_str(&self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
            Container::Webm => "webm",
            Container::Ts => "ts",
            Container::Mp3 => "mp3",
            Container::Aac => "aac",
            Container::Flac => "flac",
            Container::Wav => "wav",
            Container::Jpeg => "jpeg",
            Container::Png => "png",
            Container::Gif => "gif",
            Container::Webp => "webp",
            Container::AnnexB => "annexb",
        }
    }
}

/// Elementary video codec, identified from container sample entries / CodecIDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
    Vp8,
    Vp9,
    Av1,
    Mpeg4Part2,
    Mpeg2,
    Unknown,
}

impl VideoCodec {
    pub fn as_str(&self) -> &'static str {
        match self {
            VideoCodec::H264 => "h264",
            VideoCodec::Hevc => "hevc",
            VideoCodec::Vp8 => "vp8",
            VideoCodec::Vp9 => "vp9",
            VideoCodec::Av1 => "av1",
            VideoCodec::Mpeg4Part2 => "mpeg4-part2",
            VideoCodec::Mpeg2 => "mpeg2",
            VideoCodec::Unknown => "unknown",
        }
    }
}

/// Elementary audio codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Mp3,
    Flac,
    Opus,
    Vorbis,
    Pcm,
    Unknown,
}

impl AudioCodec {
    pub fn as_str(&self) -> &'static str {
        match self {
            AudioCodec::Aac => "aac",
            AudioCodec::Mp3 => "mp3",
            AudioCodec::Flac => "flac",
            AudioCodec::Opus => "opus",
            AudioCodec::Vorbis => "vorbis",
            AudioCodec::Pcm => "pcm",
            AudioCodec::Unknown => "unknown",
        }
    }
}

/// Video stream properties discovered from the container.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub codec: VideoCodec,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Frames per second, when derivable (e.g. `stts` / sample deltas).
    pub fps: Option<f64>,
    /// Codec profile (e.g. `"High"`, `"Main"`), from `avcC`/`hvcC` when present.
    pub profile: Option<String>,
    /// Codec level (e.g. `41` = 4.1), from `avcC`/`hvcC` when present.
    pub level: Option<u8>,
    /// `true` when the bitstream is 10-bit (e.g. HEVC Main10 / `P010`).
    pub ten_bit: Option<bool>,
}

impl VideoInfo {
    pub fn new(codec: VideoCodec) -> Self {
        Self {
            codec,
            width: None,
            height: None,
            fps: None,
            profile: None,
            level: None,
            ten_bit: None,
        }
    }
}

/// Audio stream properties discovered from the container.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    pub codec: AudioCodec,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub bit_depth: Option<u16>,
}

impl AudioInfo {
    pub fn new(codec: AudioCodec) -> Self {
        Self {
            codec,
            channels: None,
            sample_rate: None,
            bitrate_kbps: None,
            bit_depth: None,
        }
    }
}

/// Still image properties discovered from container headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillImageInfo {
    pub width: u32,
    pub height: u32,
    /// EXIF orientation value (1–8) when discovered.
    pub orientation: Option<u16>,
    /// `true` if the container indicates animated image frames (e.g. animated GIF, WebP, APNG).
    pub animated: bool,
}

impl StillImageInfo {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            orientation: None,
            animated: false,
        }
    }
}

/// The result of probing a media byte stream.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaDescriptor {
    pub container: Container,
    /// Major brand (`ftyp`) or EBML `DocType`, when present.
    pub brand: Option<String>,
    /// Total duration in milliseconds, when derivable.
    pub duration_ms: Option<u64>,
    pub video: Option<VideoInfo>,
    pub audio: Option<AudioInfo>,
    /// Still image information if container is an image.
    pub image: Option<StillImageInfo>,
    /// Initial Program Clock Reference (PCR) value (e.g. for MPEG-TS streams).
    pub pcr: Option<u64>,
}

impl MediaDescriptor {
    pub fn new(container: Container) -> Self {
        Self {
            container,
            brand: None,
            duration_ms: None,
            video: None,
            audio: None,
            image: None,
            pcr: None,
        }
    }
}
