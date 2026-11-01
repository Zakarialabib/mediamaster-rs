//! Matroska/WebM EBML parser — extracts DocType, duration, tracks, codec,
//! and dimensions from the EBML header + Segment.

use crate::error::ProbeError;
use crate::types::{AudioCodec, AudioInfo, Container, MediaDescriptor, VideoCodec, VideoInfo};

// EBML element IDs (VINT values, marker bits included).
const EBML_HEADER: u64 = 0x1A45DFA3;
const DOCTYPE: u64 = 0x4282;
const SEGMENT: u64 = 0x18538067;
const INFO: u64 = 0x1549A966;
const TIMECODE_SCALE: u64 = 0x2AD7B1;
const DURATION: u64 = 0x4489;
const TRACKS: u64 = 0x1654AE6B;
const TRACK_ENTRY: u64 = 0xAE;
const TRACK_TYPE: u64 = 0x83;
const CODEC_ID: u64 = 0x86;
const VIDEO: u64 = 0xE0;
const PIXEL_WIDTH: u64 = 0xB0;
const PIXEL_HEIGHT: u64 = 0xBA;
const AUDIO: u64 = 0xE1;
const SAMPLING_FREQUENCY: u64 = 0xB5;
const CHANNELS: u64 = 0x9F;

/// Sub-elements we recurse into.
const CONTAINERS: [u64; 6] = [EBML_HEADER, SEGMENT, INFO, TRACKS, VIDEO, AUDIO];

pub(crate) fn probe_ebml(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    // Must begin with the EBML magic.
    if data.len() < 4 || data[0..4] != [0x1A, 0x45, 0xDF, 0xA3] {
        return Err(ProbeError::UnrecognizedContainer);
    }
    let mut ctx = Ctx {
        desc: MediaDescriptor::new(Container::Mkv),
        ..Ctx::default()
    };
    walk(data, &mut ctx)?;
    if let Some(ticks) = ctx.duration_ticks {
        let ms = (ticks * ctx.timecode_scale as f64 / 1_000_000.0).round() as u64;
        ctx.desc.duration_ms = Some(ms);
    }
    Ok(ctx.into_desc())
}

#[derive(Default)]
struct Ctx {
    desc: MediaDescriptor,
    timecode_scale: u64, // ns per tick; Matroska default 1_000_000 (=1ms).
    duration_ticks: Option<f64>,
    track: Option<Track>,
    saw_doctype: bool,
}

impl Ctx {
    fn into_desc(mut self) -> MediaDescriptor {
        if !self.saw_doctype {
            self.desc.container = Container::Mkv;
        }
        self.desc
    }
}

#[derive(Default)]
struct Track {
    is_video: bool,
    codec_id: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u64>,
    sample_rate: Option<f64>,
}

impl Ctx {
    fn timecode_scale(&mut self) {
        if self.timecode_scale == 0 {
            self.timecode_scale = 1_000_000;
        }
    }
}

fn walk(data: &[u8], ctx: &mut Ctx) -> Result<(), ProbeError> {
    let mut off = 0usize;
    while off < data.len() {
        let Some((id, id_len)) = read_id(data, off) else {
            break;
        };
        let Some((size, size_len)) = read_size(data, off + id_len) else {
            break;
        };
        let content_start = off + id_len + size_len;
        let content_end = if size == u64::MAX {
            data.len()
        } else {
            let end = content_start + size as usize;
            if end > data.len() {
                return Err(ProbeError::Truncated("mkv"));
            }
            end
        };
        if content_start > data.len() {
            return Err(ProbeError::Truncated("mkv"));
        }
        let content = &data[content_start..content_end];
        handle(id, content, ctx)?;
        off = content_end;
    }
    Ok(())
}

fn handle(id: u64, content: &[u8], ctx: &mut Ctx) -> Result<(), ProbeError> {
    if CONTAINERS.contains(&id) {
        return walk(content, ctx);
    }
    match id {
        DOCTYPE => {
            let dt = String::from_utf8_lossy(content).into_owned();
            ctx.saw_doctype = true;
            ctx.desc.container = if dt == "webm" {
                Container::Webm
            } else {
                Container::Mkv
            };
            ctx.desc.brand = Some(dt);
        }
        TIMECODE_SCALE => {
            if let Some(v) = uint(content) {
                ctx.timecode_scale = v;
            }
        }
        DURATION => {
            ctx.duration_ticks = float(content);
        }
        TRACK_ENTRY => {
            ctx.track = Some(Track::default());
            walk(content, ctx)?;
            if let Some(track) = ctx.track.take() {
                commit_track(track, ctx);
            }
        }
        TRACK_TYPE => {
            if let Some(t) = ctx.track.as_mut() {
                // 1 = video, 2 = audio.
                t.is_video = uint(content) == Some(1);
            }
        }
        CODEC_ID => {
            if let Some(t) = ctx.track.as_mut() {
                t.codec_id = Some(String::from_utf8_lossy(content).into_owned());
            }
        }
        PIXEL_WIDTH => {
            if let Some(t) = ctx.track.as_mut() {
                t.width = uint(content).map(|v| v as u32);
            }
        }
        PIXEL_HEIGHT => {
            if let Some(t) = ctx.track.as_mut() {
                t.height = uint(content).map(|v| v as u32);
            }
        }
        CHANNELS => {
            if let Some(t) = ctx.track.as_mut() {
                t.channels = uint(content);
            }
        }
        SAMPLING_FREQUENCY => {
            if let Some(t) = ctx.track.as_mut() {
                t.sample_rate = float(content);
            }
        }
        _ => {}
    }
    Ok(())
}

fn commit_track(track: Track, ctx: &mut Ctx) {
    if track.is_video {
        let mut v = VideoInfo::new(
            track
                .codec_id
                .as_deref()
                .map(video_codec_from_id)
                .unwrap_or(VideoCodec::Unknown),
        );
        v.width = track.width;
        v.height = track.height;
        if let Some(id) = &track.codec_id {
            if id == "V_MPEGH/ISO/HEVC" {
                v.ten_bit = Some(true); // conservative: HEVC in MKV often Main10
            }
        }
        ctx.desc.video = Some(v);
    } else if track.codec_id.is_some() || track.channels.is_some() {
        let mut a = AudioInfo::new(
            track
                .codec_id
                .as_deref()
                .map(audio_codec_from_id)
                .unwrap_or(AudioCodec::Unknown),
        );
        a.channels = track.channels.map(|c| c as u16);
        a.sample_rate = track.sample_rate.map(|r| r as u32);
        ctx.desc.audio = Some(a);
    }
}

fn video_codec_from_id(id: &str) -> VideoCodec {
    match id {
        "V_MPEG4/ISO/AVC" => VideoCodec::H264,
        "V_MPEGH/ISO/HEVC" => VideoCodec::Hevc,
        "V_VP8" => VideoCodec::Vp8,
        "V_VP9" => VideoCodec::Vp9,
        "V_AV1" => VideoCodec::Av1,
        "V_MPEG4/ISO/ASP" => VideoCodec::Mpeg4Part2,
        "V_MPEG2" => VideoCodec::Mpeg2,
        _ => VideoCodec::Unknown,
    }
}

fn audio_codec_from_id(id: &str) -> AudioCodec {
    match id {
        "A_AAC" | "A_AAC/MPEG4/LC" => AudioCodec::Aac,
        "A_MPEG/L3" => AudioCodec::Mp3,
        "A_FLAC" => AudioCodec::Flac,
        "A_OPUS" => AudioCodec::Opus,
        "A_VORBIS" => AudioCodec::Vorbis,
        "A_PCM/INT/LIT" | "A_PCM/FLOAT/IEEE" => AudioCodec::Pcm,
        _ => AudioCodec::Unknown,
    }
}

/// Read an EBML element ID (VINT, marker bits kept) at `off`.
fn read_id(data: &[u8], off: usize) -> Option<(u64, usize)> {
    if off >= data.len() {
        return None;
    }
    let len = vint_len(data[off])?;
    if off + len > data.len() {
        return None;
    }
    let mut id = 0u64;
    for &b in &data[off..off + len] {
        id = (id << 8) | b as u64;
    }
    Some((id, len))
}

/// Read an EBML element size (VINT, marker bits stripped) at `off`.
/// Returns `u64::MAX` to signal an "unknown size" (extends to end of parent).
fn read_size(data: &[u8], off: usize) -> Option<(u64, usize)> {
    if off >= data.len() {
        return None;
    }
    let first = data[off];
    let len = vint_len(first)?;
    if off + len > data.len() {
        return None;
    }
    let mut value = (first & (0xFFu8 >> len)) as u64;
    for &b in &data[off + 1..off + len] {
        value = (value << 8) | b as u64;
    }
    let all_ones = (1u64 << (7 * len)) - 1;
    if value == all_ones {
        return Some((u64::MAX, len));
    }
    Some((value, len))
}

/// Length (in bytes) of a VINT whose first byte is `first`.
fn vint_len(first: u8) -> Option<usize> {
    if first & 0x80 != 0 {
        Some(1)
    } else if first & 0x40 != 0 {
        Some(2)
    } else if first & 0x20 != 0 {
        Some(3)
    } else if first & 0x10 != 0 {
        Some(4)
    } else if first & 0x08 != 0 {
        Some(5)
    } else if first & 0x04 != 0 {
        Some(6)
    } else if first & 0x02 != 0 {
        Some(7)
    } else if first & 0x01 != 0 {
        Some(8)
    } else {
        None
    }
}

fn uint(content: &[u8]) -> Option<u64> {
    if content.is_empty() || content.len() > 8 {
        return None;
    }
    let mut v = 0u64;
    for &b in content {
        v = (v << 8) | b as u64;
    }
    Some(v)
}

fn float(content: &[u8]) -> Option<f64> {
    match content.len() {
        8 => Some(f64::from_bits(be64(content))),
        4 => Some(f32::from_bits(be32(content)) as f64),
        _ => None,
    }
}

fn be32(d: &[u8]) -> u32 {
    u32::from_be_bytes([d[0], d[1], d[2], d[3]])
}

fn be64(d: &[u8]) -> u64 {
    u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
}
