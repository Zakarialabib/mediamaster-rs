//! Image format parsers: JPEG (SOF + EXIF orientation), PNG (IHDR + acTL animation),
//! GIF (LSD + animation check), and WebP (VP8/VP8L/VP8X chunks + animation).

use crate::error::ProbeError;
use crate::types::{Container, MediaDescriptor, StillImageInfo};

/// Probe a JPEG image.
pub fn probe_jpeg(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    if data.len() < 2 {
        return Err(ProbeError::Truncated("jpeg"));
    }
    // JPEG SOI: 0xFF, 0xD8
    if data[0] != 0xFF || data[1] != 0xD8 {
        return Err(ProbeError::UnrecognizedContainer);
    }

    let mut offset = 2usize;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut orientation: Option<u16> = None;

    while offset < data.len() {
        // Find next marker: 0xFF followed by non-0xFF and non-0x00
        if data[offset] != 0xFF {
            offset += 1;
            continue;
        }
        while offset < data.len() && data[offset] == 0xFF {
            offset += 1;
        }
        if offset >= data.len() {
            break;
        }
        let marker = data[offset];
        offset += 1;

        if marker == 0xD9 {
            // EOI (End of image)
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            // Standalone markers without payload
            continue;
        }

        if offset + 2 > data.len() {
            return Err(ProbeError::Truncated("jpeg marker length"));
        }
        let marker_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
        if marker_len < 2 {
            return Err(ProbeError::malformed("jpeg", "invalid marker length"));
        }
        let payload_start = offset + 2;
        let payload_end = offset + marker_len;
        if payload_end > data.len() {
            return Err(ProbeError::Truncated("jpeg segment"));
        }
        let payload = &data[payload_start..payload_end];

        // SOF markers: SOF0..SOF15 except DHT(0xC4), JPG(0xC8), DAC(0xCC)
        // 0xC0..0xC3, 0xC5..0xC7, 0xC9..0xCB, 0xCD..0xCF
        let is_sof = (0xC0..=0xC3).contains(&marker)
            || (0xC5..=0xC7).contains(&marker)
            || (0xC9..=0xCB).contains(&marker)
            || (0xCD..=0xCF).contains(&marker);

        if is_sof {
            // SOF payload: precision(1), height(2 BE), width(2 BE)
            if payload.len() >= 5 {
                let h = u16::from_be_bytes([payload[1], payload[2]]) as u32;
                let w = u16::from_be_bytes([payload[3], payload[4]]) as u32;
                height = Some(h);
                width = Some(w);
            }
        } else if marker == 0xE1 && orientation.is_none() {
            // APP1 (Exif) marker: payload starts with "Exif\0\0"
            if payload.len() >= 14 && &payload[0..6] == b"Exif\0\0" {
                let tiff_data = &payload[6..];
                orientation = parse_exif_orientation(tiff_data);
            }
        }

        offset = payload_end;
        if width.is_some() && orientation.is_some() {
            break;
        }
    }

    match (width, height) {
        (Some(w), Some(h)) => {
            let mut info = StillImageInfo::new(w, h);
            info.orientation = orientation;
            let mut desc = MediaDescriptor::new(Container::Jpeg);
            desc.image = Some(info);
            Ok(desc)
        }
        _ => Err(ProbeError::Truncated("jpeg SOF")),
    }
}

/// Parse TIFF IFD0 from Exif block to find Orientation (Tag 0x0112).
fn parse_exif_orientation(tiff: &[u8]) -> Option<u16> {
    if tiff.len() < 8 {
        return None;
    }
    let is_le = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };

    let read_u16 = |slice: &[u8]| -> Option<u16> {
        if slice.len() < 2 {
            None
        } else if is_le {
            Some(u16::from_le_bytes([slice[0], slice[1]]))
        } else {
            Some(u16::from_be_bytes([slice[0], slice[1]]))
        }
    };

    let read_u32 = |slice: &[u8]| -> Option<u32> {
        if slice.len() < 4 {
            None
        } else if is_le {
            Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
        } else {
            Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
        }
    };

    let magic = read_u16(&tiff[2..4])?;
    if magic != 42 {
        return None;
    }

    let ifd0_offset = read_u32(&tiff[4..8])? as usize;
    if ifd0_offset + 2 > tiff.len() {
        return None;
    }

    let num_entries = read_u16(&tiff[ifd0_offset..ifd0_offset + 2])? as usize;
    let mut entry_offset = ifd0_offset + 2;

    for _ in 0..num_entries {
        if entry_offset + 12 > tiff.len() {
            break;
        }
        let entry = &tiff[entry_offset..entry_offset + 12];
        let tag = read_u16(&entry[0..2])?;
        if tag == 0x0112 {
            // Orientation tag
            let format = read_u16(&entry[2..4])?;
            if format == 3 {
                // SHORT (16-bit)
                let val = read_u16(&entry[8..10])?;
                return Some(val);
            }
        }
        entry_offset += 12;
    }

    None
}

/// Probe a PNG image.
pub fn probe_png(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 8 {
        return Err(ProbeError::Truncated("png"));
    }
    if data[0..8] != PNG_MAGIC {
        return Err(ProbeError::UnrecognizedContainer);
    }

    let mut offset = 8usize;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut animated = false;

    while offset + 8 <= data.len() {
        let chunk_len = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let chunk_type = &data[offset + 4..offset + 8];

        let content_start = offset + 8;
        let content_end = content_start + chunk_len;
        if content_end + 4 > data.len() {
            // CRC is 4 bytes
            if chunk_type == b"IHDR" {
                return Err(ProbeError::Truncated("png ihdr"));
            }
            break;
        }

        if chunk_type == b"IHDR" {
            if chunk_len < 8 {
                return Err(ProbeError::malformed("png", "IHDR chunk too small"));
            }
            let w = u32::from_be_bytes([
                data[content_start],
                data[content_start + 1],
                data[content_start + 2],
                data[content_start + 3],
            ]);
            let h = u32::from_be_bytes([
                data[content_start + 4],
                data[content_start + 5],
                data[content_start + 6],
                data[content_start + 7],
            ]);
            width = Some(w);
            height = Some(h);
        } else if chunk_type == b"acTL" {
            // APNG animation control chunk
            animated = true;
        } else if chunk_type == b"IEND" {
            break;
        }

        offset = content_end + 4; // skip CRC
    }

    match (width, height) {
        (Some(w), Some(h)) => {
            let mut info = StillImageInfo::new(w, h);
            info.animated = animated;
            let mut desc = MediaDescriptor::new(Container::Png);
            desc.image = Some(info);
            Ok(desc)
        }
        _ => Err(ProbeError::Truncated("png")),
    }
}

/// Probe a GIF image.
pub fn probe_gif(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    if data.len() < 6 {
        return Err(ProbeError::Truncated("gif"));
    }
    if &data[0..6] != b"GIF87a" && &data[0..6] != b"GIF89a" {
        return Err(ProbeError::UnrecognizedContainer);
    }
    if data.len() < 13 {
        return Err(ProbeError::Truncated("gif lsd"));
    }

    // Logical Screen Descriptor: width(2 LE), height(2 LE), packed(1), bg_color(1), aspect(1)
    let width = u16::from_le_bytes([data[6], data[7]]) as u32;
    let height = u16::from_le_bytes([data[8], data[9]]) as u32;
    let packed = data[10];
    let gct_flag = (packed & 0x80) != 0;
    let gct_size_exp = packed & 0x07;

    let mut offset = 13usize;
    if gct_flag {
        let gct_len = 3 * (1 << (gct_size_exp + 1));
        if offset + gct_len > data.len() {
            return Err(ProbeError::Truncated("gif global color table"));
        }
        offset += gct_len;
    }

    // Scan blocks to count image descriptors (0x2C)
    let mut image_count = 0;
    while offset < data.len() {
        let block_intro = data[offset];
        if block_intro == 0x3B {
            // Trailer
            break;
        } else if block_intro == 0x2C {
            // Image descriptor
            image_count += 1;
            if offset + 10 > data.len() {
                break;
            }
            let lct_packed = data[offset + 9];
            offset += 10;
            if (lct_packed & 0x80) != 0 {
                let lct_size = 3 * (1 << ((lct_packed & 0x07) + 1));
                offset += lct_size;
            }
            if offset >= data.len() {
                break;
            }
            // Skip LZW minimum code size
            offset += 1;
            // Skip data sub-blocks
            while offset < data.len() {
                let sub_len = data[offset] as usize;
                offset += 1;
                if sub_len == 0 {
                    break;
                }
                offset += sub_len;
            }
        } else if block_intro == 0x21 {
            // Extension
            offset += 1;
            if offset >= data.len() {
                break;
            }
            let _ext_label = data[offset];
            offset += 1;
            // Skip extension sub-blocks
            while offset < data.len() {
                let sub_len = data[offset] as usize;
                offset += 1;
                if sub_len == 0 {
                    break;
                }
                offset += sub_len;
            }
        } else {
            // Unknown or sync lost
            offset += 1;
        }

        if image_count > 1 {
            break;
        }
    }

    let mut info = StillImageInfo::new(width, height);
    info.animated = image_count > 1;

    let mut desc = MediaDescriptor::new(Container::Gif);
    desc.image = Some(info);
    Ok(desc)
}

/// Probe a WebP image.
pub fn probe_webp(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    if data.len() < 12 {
        return Err(ProbeError::Truncated("webp"));
    }
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return Err(ProbeError::UnrecognizedContainer);
    }

    let mut offset = 12usize;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut animated = false;

    while offset + 8 <= data.len() {
        let chunk_fourcc = &data[offset..offset + 4];
        let chunk_len = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;

        let content_start = offset + 8;
        let content_end = content_start + chunk_len;
        if content_end > data.len() {
            return Err(ProbeError::Truncated("webp chunk"));
        }
        let chunk_data = &data[content_start..content_end];

        if chunk_fourcc == b"VP8 " {
            // VP8 lossy: Keyframe header has 3-byte frame tag, then 3-byte start code 0x9D 0x01 0x2A
            if chunk_data.len() >= 10 && chunk_data[3..6] == [0x9D, 0x01, 0x2A] {
                let w = (u16::from_le_bytes([chunk_data[6], chunk_data[7]]) & 0x3FFF) as u32;
                let h = (u16::from_le_bytes([chunk_data[8], chunk_data[9]]) & 0x3FFF) as u32;
                width = Some(w);
                height = Some(h);
            }
        } else if chunk_fourcc == b"VP8L" {
            // VP8L lossless: 1 byte signature (0x2F), then 14-bit width-1, 14-bit height-1
            if chunk_data.len() >= 5 && chunk_data[0] == 0x2F {
                let b1 = chunk_data[1] as u32;
                let b2 = chunk_data[2] as u32;
                let b3 = chunk_data[3] as u32;
                let b4 = chunk_data[4] as u32;

                let w = (b1 | ((b2 & 0x3F) << 8)) + 1;
                let h = ((b2 >> 6) | (b3 << 2) | ((b4 & 0x0F) << 10)) + 1;
                width = Some(w);
                height = Some(h);
            }
        } else if chunk_fourcc == b"VP8X" {
            // VP8X extended: flags(4), canvas_width_minus_one(3 LE), canvas_height_minus_one(3 LE)
            if chunk_data.len() >= 10 {
                let flags = chunk_data[0];
                animated = (flags & 0x02) != 0; // bit 1: Animation flag
                let w = (chunk_data[4] as u32
                    | ((chunk_data[5] as u32) << 8)
                    | ((chunk_data[6] as u32) << 16))
                    + 1;
                let h = (chunk_data[7] as u32
                    | ((chunk_data[8] as u32) << 8)
                    | ((chunk_data[9] as u32) << 16))
                    + 1;
                width = Some(w);
                height = Some(h);
            }
        }

        // Padded to even size
        let padded = (chunk_len + 1) & !1;
        offset = content_start + padded;
    }

    match (width, height) {
        (Some(w), Some(h)) => {
            let mut info = StillImageInfo::new(w, h);
            info.animated = animated;
            let mut desc = MediaDescriptor::new(Container::Webp);
            desc.image = Some(info);
            Ok(desc)
        }
        _ => Err(ProbeError::Truncated("webp")),
    }
}
