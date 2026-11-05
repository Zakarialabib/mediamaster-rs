//! Pure audio container parsers: MP3, ADTS (AAC), FLAC, and WAV/RF64.

use crate::error::ProbeError;
use crate::types::{AudioCodec, AudioInfo, Container, MediaDescriptor};

/// Probes an MP3 stream (ID3v2 header skip + first MPEG Audio Layer III frame header).
pub fn probe_mp3(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }

    let mut offset = 0;

    // Check for ID3v2 header: "ID3" + version(2) + flags(1) + size(4 syncsafe)
    if data.len() >= 10 && &data[0..3] == b"ID3" {
        let size = read_syncsafe_u32(&data[6..10]);
        let mut total_id3_len = 10 + size as usize;
        let flags = data[5];
        // Footer present if bit 4 is set
        if (flags & 0x10) != 0 {
            total_id3_len += 10;
        }
        if total_id3_len > data.len() {
            return Err(ProbeError::Truncated("mp3 id3v2"));
        }
        offset = total_id3_len;
    }

    // Search for MPEG audio sync: 11 bits set (0xFF followed by high 3 bits set in next byte)
    while offset + 4 <= data.len() {
        if data[offset] == 0xFF && (data[offset + 1] & 0xE0) == 0xE0 {
            // Check frame header
            if let Some((audio_info, _frame_len)) = parse_mp3_frame(&data[offset..]) {
                // If possible, estimate duration based on file size and bitrate
                let mut desc = MediaDescriptor::new(Container::Mp3);
                if let Some(bitrate) = audio_info.bitrate_kbps {
                    if bitrate > 0 {
                        let audio_bytes = (data.len() - offset) as u64;
                        let duration_ms = (audio_bytes * 8) / (bitrate as u64);
                        desc.duration_ms = Some(duration_ms);
                    }
                }
                desc.audio = Some(audio_info);
                return Ok(desc);
            } else {
                offset += 1;
            }
        } else {
            offset += 1;
        }
    }

    if offset >= data.len() {
        Err(ProbeError::Truncated("mp3"))
    } else {
        Err(ProbeError::malformed(
            "mp3",
            "no valid mpeg audio frame header found",
        ))
    }
}

fn parse_mp3_frame(data: &[u8]) -> Option<(AudioInfo, usize)> {
    if data.len() < 4 {
        return None;
    }
    let b1 = data[1];
    let b2 = data[2];
    let b3 = data[3];

    let version_bits = (b1 >> 3) & 0x03;
    let layer_bits = (b1 >> 1) & 0x03;

    // Version: 00 = 2.5, 01 = reserved, 10 = 2, 11 = 1
    // Layer: 00 = reserved, 01 = Layer III, 10 = Layer II, 11 = Layer I
    if version_bits == 0x01 || layer_bits == 0x00 {
        return None;
    }

    let mpeg_version = match version_bits {
        0x00 => 25, // MPEG 2.5
        0x02 => 20, // MPEG 2
        0x03 => 10, // MPEG 1
        _ => return None,
    };
    let layer = match layer_bits {
        0x01 => 3,
        0x02 => 2,
        0x03 => 1,
        _ => return None,
    };

    let bitrate_idx = ((b2 >> 4) & 0x0F) as usize;
    let srate_idx = ((b2 >> 2) & 0x03) as usize;
    let padding = ((b2 >> 1) & 0x01) as usize;

    if bitrate_idx == 0 || bitrate_idx == 15 || srate_idx == 3 {
        return None;
    }

    let sample_rate: u32 = match (mpeg_version, srate_idx) {
        (10, 0) => 44100,
        (10, 1) => 48000,
        (10, 2) => 32000,
        (20, 0) => 22050,
        (20, 1) => 24000,
        (20, 2) => 16000,
        (25, 0) => 11025,
        (25, 1) => 12000,
        (25, 2) => 8000,
        _ => return None,
    };

    // Bitrate table in kbps
    let bitrate_kbps = match (mpeg_version, layer) {
        (10, 1) => [
            0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ][bitrate_idx],
        (10, 2) => [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ][bitrate_idx],
        (10, 3) => [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ][bitrate_idx],
        (_, 1) => [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ][bitrate_idx],
        (_, _) => [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160][bitrate_idx],
    };

    let channel_mode = (b3 >> 6) & 0x03;
    let channels = if channel_mode == 3 { 1 } else { 2 };

    let frame_len = if layer == 1 {
        (12 * bitrate_kbps * 1000 / (sample_rate as usize)) + padding * 4
    } else if layer == 2 || (layer == 3 && mpeg_version == 10) {
        (144 * bitrate_kbps * 1000 / (sample_rate as usize)) + padding
    } else {
        (72 * bitrate_kbps * 1000 / (sample_rate as usize)) + padding
    };

    let mut info = AudioInfo::new(AudioCodec::Mp3);
    info.channels = Some(channels);
    info.sample_rate = Some(sample_rate);
    info.bitrate_kbps = Some(bitrate_kbps as u32);

    Some((info, frame_len))
}

fn read_syncsafe_u32(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32 & 0x7F) << 21)
        | ((bytes[1] as u32 & 0x7F) << 14)
        | ((bytes[2] as u32 & 0x7F) << 7)
        | (bytes[3] as u32 & 0x7F)
}

/// Probes an ADTS (AAC) stream.
pub fn probe_adts(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    if data.len() < 7 {
        return Err(ProbeError::Truncated("adts"));
    }

    // Check ADTS syncword: 12 bits set 0xFFF
    if data[0] != 0xFF || (data[1] & 0xF0) != 0xF0 {
        return Err(ProbeError::UnrecognizedContainer);
    }

    // Byte 1: syncword low 4 bits (1111), ID (1 bit, 0=MPEG-4, 1=MPEG-2), layer (2 bits, must be 00), protection_absent (1 bit)
    let layer = (data[1] >> 1) & 0x03;
    if layer != 0 {
        return Err(ProbeError::malformed("adts", "invalid adts layer"));
    }

    // Byte 2: profile (2 bits), sampling_frequency_index (4 bits), private_bit (1 bit), channel_configuration high 1 bit
    let _profile = (data[2] >> 6) & 0x03;
    let srate_idx = (data[2] >> 2) & 0x0F;
    let chan_hi = (data[2] & 0x01) << 2;

    // Byte 3: channel_configuration low 2 bits, original/copy, home, copyright...
    let chan_lo = (data[3] >> 6) & 0x03;
    let channel_config = chan_hi | chan_lo;

    let sample_rate = match srate_idx {
        0 => 96000,
        1 => 88200,
        2 => 64000,
        3 => 48000,
        4 => 44100,
        5 => 32000,
        6 => 24000,
        7 => 22050,
        8 => 16000,
        9 => 12000,
        10 => 11025,
        11 => 8000,
        12 => 7350,
        _ => {
            return Err(ProbeError::malformed(
                "adts",
                "unsupported sample rate index",
            ))
        }
    };

    let channels = match channel_config {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 5,
        6 => 6,
        7 => 8,
        _ => {
            return Err(ProbeError::malformed(
                "adts",
                "unsupported channel configuration",
            ))
        }
    };

    let frame_length = ((data[3] as usize & 0x03) << 11)
        | ((data[4] as usize) << 3)
        | ((data[5] as usize) >> 5);

    if frame_length < 7 {
        return Err(ProbeError::malformed("adts", "invalid frame length"));
    }

    let mut audio = AudioInfo::new(AudioCodec::Aac);
    audio.channels = Some(channels);
    audio.sample_rate = Some(sample_rate);

    let mut desc = MediaDescriptor::new(Container::Aac);
    desc.audio = Some(audio);
    Ok(desc)
}

/// Probes a FLAC byte stream.
pub fn probe_flac(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    if data.len() < 4 {
        return Err(ProbeError::Truncated("flac"));
    }
    if &data[0..4] != b"fLaC" {
        return Err(ProbeError::UnrecognizedContainer);
    }

    if data.len() < 4 + 4 + 34 {
        return Err(ProbeError::Truncated("flac streaminfo"));
    }

    // Metadata block header: 1 bit is_last, 7 bits block_type, 24 bits block_length
    let header_byte = data[4];
    let block_type = header_byte & 0x7F;
    let block_len = ((data[5] as usize) << 16) | ((data[6] as usize) << 8) | (data[7] as usize);

    if block_type != 0 {
        return Err(ProbeError::malformed(
            "flac",
            "first metadata block must be STREAMINFO",
        ));
    }
    if block_len < 34 {
        return Err(ProbeError::malformed(
            "flac",
            "STREAMINFO block length too short",
        ));
    }

    let info_bytes = &data[8..8 + 34];
    // STREAMINFO:
    // min_block_size: 16 bits (0..2)
    // max_block_size: 16 bits (2..4)
    // min_frame_size: 24 bits (4..7)
    // max_frame_size: 24 bits (7..10)
    // sample_rate (20 bits), channels (3 bits, 0=1chan), bits_per_sample (5 bits, 0=1bit), total_samples (36 bits) -> spans bytes 10..18
    let b10 = info_bytes[10] as u32;
    let b11 = info_bytes[11] as u32;
    let b12 = info_bytes[12] as u32;
    let b13 = info_bytes[13] as u32;
    let sample_rate = (b10 << 12) | (b11 << 4) | (b12 >> 4);
    let channels = (((b12 >> 1) & 0x07) + 1) as u16;
    let bits_per_sample = ((((b12 & 0x01) << 4) | (b13 >> 4)) + 1) as u16;

    let b14 = info_bytes[14] as u64;
    let b15 = info_bytes[15] as u64;
    let b16 = info_bytes[16] as u64;
    let b17 = info_bytes[17] as u64;
    let total_samples =
        (((b13 & 0x0F) as u64) << 32) | (b14 << 24) | (b15 << 16) | (b16 << 8) | b17;

    let mut audio = AudioInfo::new(AudioCodec::Flac);
    audio.channels = Some(channels);
    audio.sample_rate = Some(sample_rate);
    audio.bit_depth = Some(bits_per_sample);

    let mut desc = MediaDescriptor::new(Container::Flac);
    if sample_rate > 0 && total_samples > 0 {
        desc.duration_ms = Some(total_samples * 1000 / sample_rate as u64);
    }
    desc.audio = Some(audio);
    Ok(desc)
}

/// Probes a WAV or RF64 stream.
pub fn probe_wav(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    if data.len() < 12 {
        return Err(ProbeError::Truncated("wav"));
    }

    let is_riff = &data[0..4] == b"RIFF";
    let is_rf64 = &data[0..4] == b"RF64";

    if (!is_riff && !is_rf64) || &data[8..12] != b"WAVE" {
        return Err(ProbeError::UnrecognizedContainer);
    }

    let mut offset = 12usize;
    let mut audio_info = AudioInfo::new(AudioCodec::Pcm);
    let mut data_size_bytes: Option<u64> = None;
    let mut bytes_per_sec: u32 = 0;
    let mut found_fmt = false;

    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;

        let chunk_data_start = offset + 8;
        let chunk_data_end = chunk_data_start + chunk_size;

        if chunk_id == b"ds64" && is_rf64 {
            // RF64 data size chunk
            if chunk_data_start + 24 <= data.len() {
                let riff_size = u64::from_le_bytes(
                    data[chunk_data_start..chunk_data_start + 8]
                        .try_into()
                        .unwrap(),
                );
                let data_size = u64::from_le_bytes(
                    data[chunk_data_start + 8..chunk_data_start + 16]
                        .try_into()
                        .unwrap(),
                );
                let _sample_count = u64::from_le_bytes(
                    data[chunk_data_start + 16..chunk_data_start + 24]
                        .try_into()
                        .unwrap(),
                );
                let _ = riff_size;
                data_size_bytes = Some(data_size);
            }
        } else if chunk_id == b"fmt " {
            if chunk_data_start + 16 > data.len() {
                return Err(ProbeError::Truncated("wav fmt"));
            }
            let fmt_data = &data[chunk_data_start..data.len().min(chunk_data_end)];
            if fmt_data.len() < 16 {
                return Err(ProbeError::Truncated("wav fmt"));
            }
            let format_tag = u16::from_le_bytes([fmt_data[0], fmt_data[1]]);
            let channels = u16::from_le_bytes([fmt_data[2], fmt_data[3]]);
            let sample_rate =
                u32::from_le_bytes([fmt_data[4], fmt_data[5], fmt_data[6], fmt_data[7]]);
            bytes_per_sec =
                u32::from_le_bytes([fmt_data[8], fmt_data[9], fmt_data[10], fmt_data[11]]);
            let bits_per_sample = u16::from_le_bytes([fmt_data[14], fmt_data[15]]);

            let codec = match format_tag {
                1 => AudioCodec::Pcm, // PCM
                3 => AudioCodec::Pcm, // IEEE Float
                0xFFFE => {
                    // WAVEFORMATEXTENSIBLE
                    if fmt_data.len() >= 40 {
                        let sub_format = u16::from_le_bytes([fmt_data[24], fmt_data[25]]);
                        if sub_format == 1 || sub_format == 3 {
                            AudioCodec::Pcm
                        } else {
                            AudioCodec::Unknown
                        }
                    } else {
                        AudioCodec::Pcm
                    }
                }
                _ => AudioCodec::Unknown,
            };

            audio_info.codec = codec;
            audio_info.channels = Some(channels);
            audio_info.sample_rate = Some(sample_rate);
            audio_info.bit_depth = Some(bits_per_sample);
            found_fmt = true;
        } else if chunk_id == b"data" && data_size_bytes.is_none() {
            data_size_bytes = Some(chunk_size as u64);
        }

        // Chunks are padded to word (2-byte) boundary
        let padded_size = (chunk_size + 1) & !1;
        offset = chunk_data_start + padded_size;
    }

    if !found_fmt {
        return Err(ProbeError::Truncated("wav fmt"));
    }

    let mut desc = MediaDescriptor::new(Container::Wav);
    if let (Some(dsize), true) = (data_size_bytes, bytes_per_sec > 0) {
        desc.duration_ms = Some(dsize * 1000 / bytes_per_sec as u64);
    }
    desc.audio = Some(audio_info);
    Ok(desc)
}
