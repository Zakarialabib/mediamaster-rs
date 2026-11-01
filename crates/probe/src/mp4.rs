//! ISO-BMFF (MP4/MOV) box parser — extracts container, duration, codec,
//! dimensions, and (from `avcC`/`hvcC`) profile/level/bit-depth.

use crate::error::ProbeError;
use crate::types::{AudioCodec, AudioInfo, Container, MediaDescriptor, VideoCodec, VideoInfo};

pub(crate) fn probe_mp4(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    let mut ctx = Ctx::default();
    walk_boxes(data, &mut ctx)?;
    Ok(ctx.into_desc())
}

#[derive(Default)]
struct Ctx {
    desc: MediaDescriptor,
    video: Option<VideoInfo>,
    audio: Option<AudioInfo>,
    /// Per-track timescale/duration (from `mdhd`), used for fps fallback.
    mdhd_timescale: u32,
    mdhd_duration: u64,
}

impl Ctx {
    fn video(&mut self) -> &mut VideoInfo {
        self.video
            .get_or_insert_with(|| VideoInfo::new(VideoCodec::Unknown))
    }

    fn audio(&mut self) -> &mut AudioInfo {
        self.audio
            .get_or_insert_with(|| AudioInfo::new(AudioCodec::Unknown))
    }

    fn into_desc(mut self) -> MediaDescriptor {
        if self.desc.duration_ms.is_none() && self.mdhd_timescale > 0 {
            if self.mdhd_duration != u64::MAX && self.mdhd_duration != u32::MAX as u64 {
                self.desc.duration_ms =
                    Some(self.mdhd_duration * 1000 / self.mdhd_timescale as u64);
            }
        }
        self.desc.video = self.video;
        self.desc.audio = self.audio;
        self.desc
    }
}

/// Iterate over sibling boxes in `data`, dispatching each to `handle`.
fn walk_boxes(data: &[u8], ctx: &mut Ctx) -> Result<(), ProbeError> {
    let mut off = 0usize;
    while off + 8 <= data.len() {
        let mut size = be32(data, off) as usize;
        let btype = &data[off + 4..off + 8];
        let header_len = if size == 0 {
            // Box extends to end of buffer.
            8
        } else if size == 1 {
            if off + 16 > data.len() {
                return Err(ProbeError::Truncated("mp4"));
            }
            size = be64(data, off + 8) as usize;
            16
        } else {
            8
        };
        if size == 0 {
            size = data.len() - off;
        }
        if size < header_len || off + size > data.len() {
            return Err(ProbeError::malformed("mp4", "bad box size"));
        }
        let content = &data[off + header_len..off + size];
        handle_box(btype, content, ctx)?;
        off += size;
    }
    Ok(())
}

fn handle_box(btype: &[u8], content: &[u8], ctx: &mut Ctx) -> Result<(), ProbeError> {
    match btype {
        b"ftyp" => {
            if content.len() >= 4 {
                let brand = String::from_utf8_lossy(&content[0..4]).into_owned();
                ctx.desc.brand = Some(brand);
            }
        }
        b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts" | b"dinf" | b"udta" => {
            walk_boxes(content, ctx)?;
        }
        b"mvhd" => {
            let (timescale, duration) = read_mvhd(content)?;
            if timescale > 0 && duration != u64::MAX && duration != u32::MAX as u64 {
                ctx.desc.duration_ms = Some(duration * 1000 / timescale as u64);
            }
        }
        b"mdhd" => {
            let (timescale, duration) = read_mvhd(content)?;
            ctx.mdhd_timescale = timescale;
            ctx.mdhd_duration = duration;
        }
        b"tkhd" => read_tkhd(content, ctx)?,
        b"stsd" => read_stsd(content, ctx)?,
        b"stts" => read_stts(content, ctx),
        // `avcC`/`hvcC` are reached via `stsd` descent, but also handle them
        // directly in case they appear elsewhere.
        b"avcC" => read_avcc(content, ctx),
        b"hvcC" => read_hvcc(content, ctx),
        _ => {}
    }
    Ok(())
}

/// `mvhd`/`mdhd` share the same timescale+duration layout.
fn read_mvhd(content: &[u8]) -> Result<(u32, u64), ProbeError> {
    if content.len() < 4 {
        return Err(ProbeError::Truncated("mvhd/mdhd"));
    }
    let version = content[0];
    if version == 1 {
        if content.len() < 32 {
            return Err(ProbeError::Truncated("mvhd/mdhd v1"));
        }
        let timescale = be32(content, 20);
        let duration = be64(content, 24);
        Ok((timescale, duration))
    } else {
        if content.len() < 20 {
            return Err(ProbeError::Truncated("mvhd/mdhd v0"));
        }
        let timescale = be32(content, 12);
        let duration = be32(content, 16) as u64;
        Ok((timescale, duration))
    }
}

fn read_tkhd(content: &[u8], ctx: &mut Ctx) -> Result<(), ProbeError> {
    if content.is_empty() {
        return Err(ProbeError::Truncated("tkhd"));
    }
    let version = content[0];
    // width/height are 16.16 fixed point, at a version-dependent offset.
    let (w_off, h_off) = if version == 1 { (88, 92) } else { (76, 80) };
    if content.len() >= h_off + 4 {
        let w = be32(content, w_off) >> 16;
        let h = be32(content, h_off) >> 16;
        if w > 0 && h > 0 {
            let v = ctx.video();
            v.width = Some(w);
            v.height = Some(h);
        }
    }
    Ok(())
}

fn read_stsd(content: &[u8], ctx: &mut Ctx) -> Result<(), ProbeError> {
    if content.len() < 8 {
        return Err(ProbeError::Truncated("stsd"));
    }
    let entry_count = be32(content, 4);
    let mut off = 8usize;
    for _ in 0..entry_count {
        if off + 8 > content.len() {
            break;
        }
        let esize = be32(content, off) as usize;
        if esize < 8 || off + esize > content.len() {
            return Err(ProbeError::malformed("mp4", "bad sample entry size"));
        }
        let format = &content[off + 4..off + 8];
        let entry = &content[off..off + esize];
        handle_sample_entry(format, entry, ctx);
        off += esize;
    }
    Ok(())
}

fn handle_sample_entry(format: &[u8], entry: &[u8], ctx: &mut Ctx) {
    let codec = video_codec_from_fourcc(format);
    let is_video = !matches!(codec, VideoCodec::Unknown)
        && format != b"mp4a"
        && format != b".mp3"
        && format != b"fLaC"
        && format != b"opus";
    // Visual sample entry: width at entry[32..34], height at [34..36].
    if is_video {
        let v = ctx.video();
        v.codec = codec;
        if entry.len() >= 36 {
            let w = u16::from_be_bytes([entry[32], entry[33]]) as u32;
            let h = u16::from_be_bytes([entry[34], entry[35]]) as u32;
            if w > 0 && h > 0 {
                v.width = Some(w);
                v.height = Some(h);
            }
        }
        // Descend into child boxes (`avcC`/`hvcC`) after the fixed visual header.
        let children = entry.get(86..).unwrap_or(&[]);
        let mut off = 0usize;
        while off + 8 <= children.len() {
            let size = be32(children, off) as usize;
            let btype = &children[off + 4..off + 8];
            if size < 8 || off + size > children.len() {
                break;
            }
            let child = &children[off + 8..off + size];
            match btype {
                b"avcC" => read_avcc(child, ctx),
                b"hvcC" => read_hvcc(child, ctx),
                _ => {}
            }
            off += size;
        }
    } else {
        // Audio sample entry (`mp4a` etc.): channels at entry[24..26],
        // samplerate (16.16) at entry[32..36].
        let a = ctx.audio();
        a.codec = audio_codec_from_fourcc(format);
        if entry.len() >= 36 {
            let channels = u16::from_be_bytes([entry[24], entry[25]]);
            let rate = u32::from_be_bytes([entry[32], entry[33], entry[34], entry[35]]) >> 16;
            if channels > 0 {
                a.channels = Some(channels);
            }
            if rate > 0 {
                a.sample_rate = Some(rate);
            }
        }
    }
}

fn read_stts(content: &[u8], ctx: &mut Ctx) {
    if content.len() < 12 || ctx.mdhd_timescale == 0 || ctx.video.is_none() {
        return;
    }
    let entry_count = be32(content, 4);
    if entry_count == 0 {
        return;
    }
    // First entry: sample_count at [8..12], sample_delta at [12..16].
    let sample_delta = be32(content, 12);
    if sample_delta > 0 && ctx.mdhd_timescale > 0 {
        let fps = ctx.mdhd_timescale as f64 / sample_delta as f64;
        if fps.is_finite() && fps > 0.0 {
            if let Some(v) = ctx.video.as_mut() {
                v.fps = Some((fps * 1000.0).round() / 1000.0);
            }
        }
    }
}

fn read_avcc(content: &[u8], ctx: &mut Ctx) {
    // AVCC: version(1) profile_indication(1) compat(1) level(1) …
    if content.len() < 4 {
        return;
    }
    let profile_idc = content[1];
    let level_idc = content[3];
    let v = ctx.video();
    v.profile = Some(avc_profile_name(profile_idc).to_string());
    v.level = Some(level_idc);
    // High profiles (100/110/122/128/144…) may be 10-bit only for some; H.264
    // High10 = 110.
    if profile_idc == 110 || profile_idc == 122 || profile_idc == 144 {
        v.ten_bit = Some(true);
    } else {
        v.ten_bit = Some(false);
    }
}

fn read_hvcc(content: &[u8], ctx: &mut Ctx) {
    // HVCC: version(1) profile_tier_level(1) compat(4) constraint(6) level(1) …
    if content.len() < 13 {
        return;
    }
    let level_idc = content[12];
    let v = ctx.video();
    v.profile = Some("HEVC".to_string());
    v.level = Some(level_idc);
    // general_profile_idc lives in byte 1 (5 bits). 2 = Main10.
    let profile_idc = content[1] & 0x1f;
    v.ten_bit = Some(profile_idc == 2 || profile_idc == 4 || profile_idc == 5);
}

fn avc_profile_name(idc: u8) -> &'static str {
    match idc {
        66 => "Baseline",
        77 => "Main",
        88 => "Extended",
        100 => "High",
        110 => "High 10",
        122 => "High 4:2:2",
        144 => "High 4:4:4",
        _ => "Unknown",
    }
}

fn video_codec_from_fourcc(f: &[u8]) -> VideoCodec {
    match f {
        b"avc1" | b"avc3" => VideoCodec::H264,
        b"hev1" | b"hvc1" => VideoCodec::Hevc,
        b"vp08" => VideoCodec::Vp8,
        b"vp09" => VideoCodec::Vp9,
        b"av01" => VideoCodec::Av1,
        b"mp4v" => VideoCodec::Mpeg4Part2,
        b"mp2v" | b"mpeg" => VideoCodec::Mpeg2,
        _ => VideoCodec::Unknown,
    }
}

fn audio_codec_from_fourcc(f: &[u8]) -> AudioCodec {
    match f {
        b"mp4a" | b"enca" | b"ac-3" | b"ec-3" => AudioCodec::Aac,
        b".mp3" | b"mp3 " => AudioCodec::Mp3,
        b"fLaC" => AudioCodec::Flac,
        b"Opus" | b"opus" => AudioCodec::Opus,
        b"ac-3" => AudioCodec::Unknown,
        _ => AudioCodec::Unknown,
    }
}

fn be32(d: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

fn be64(d: &[u8], off: usize) -> u64 {
    u64::from_be_bytes([
        d[off],
        d[off + 1],
        d[off + 2],
        d[off + 3],
        d[off + 4],
        d[off + 5],
        d[off + 6],
        d[off + 7],
    ])
}
