//! MPEG-2 Transport Stream (MPEG-TS) parser.
//!
//! MPEG-TS consists of fixed 188-byte packets starting with the sync byte `0x47`.
//! This parser scans for sync byte synchronization, walks packets, extracts
//! the Program Association Table (PAT, PID 0x0000) to find Program Map Table (PMT)
//! PIDs, inspects the PMT to discover elementary stream types (mapped to
//! `VideoCodec` / `AudioCodec`), extracts the initial Program Clock Reference (PCR),
//! and handles continuity-counter discontinuities gracefully.

use std::collections::HashSet;

use crate::error::ProbeError;
use crate::types::{AudioCodec, AudioInfo, Container, MediaDescriptor, VideoCodec, VideoInfo};

const TS_PACKET_SIZE: usize = 188;
const TS_SYNC_BYTE: u8 = 0x47;

/// Check if `data` looks like an MPEG-TS stream by finding a sequence of sync bytes spaced 188 bytes apart.
pub fn is_mpegts(data: &[u8]) -> bool {
    find_sync_offset(data).is_some()
}

/// Find the offset of the first MPEG-TS packet in `data`.
/// Requires at least 2 consecutive packets (or 1 packet if data is exactly 188 bytes, or >= 188 bytes with sync).
pub fn find_sync_offset(data: &[u8]) -> Option<usize> {
    if data.len() < TS_PACKET_SIZE {
        return None;
    }
    // Search up to the first packet length for a sync byte that repeats every 188 bytes.
    let max_search = data.len().min(TS_PACKET_SIZE);
    for offset in 0..max_search {
        if data[offset] == TS_SYNC_BYTE {
            // Check if subsequent packets also start with 0x47
            let mut matches = 1;
            let mut next = offset + TS_PACKET_SIZE;
            while next < data.len() && matches < 3 {
                if data[next] == TS_SYNC_BYTE {
                    matches += 1;
                    next += TS_PACKET_SIZE;
                } else {
                    break;
                }
            }
            // If data is smaller than 2 packets, 1 sync byte at offset is enough
            if (matches >= 2) || (data.len() < TS_PACKET_SIZE * 2 && offset == 0) {
                return Some(offset);
            }
        }
    }
    None
}

/// Probes an MPEG-TS byte stream and returns a `MediaDescriptor`.
pub fn probe_ts(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }
    let start_offset = find_sync_offset(data).ok_or(ProbeError::UnrecognizedContainer)?;

    let mut desc = MediaDescriptor::new(Container::Ts);
    let mut pmt_pids: HashSet<u16> = HashSet::new();
    let mut elementary_streams: Vec<(u8, u16)> = Vec::new(); // (stream_type, elementary_pid)
    let mut initial_pcr: Option<u64> = None;

    let mut offset = start_offset;
    let mut packets_parsed = 0;

    while offset + TS_PACKET_SIZE <= data.len() {
        let packet = &data[offset..offset + TS_PACKET_SIZE];
        if packet[0] != TS_SYNC_BYTE {
            // Discontinuity or corrupted packet: resync
            if let Some(resync) = find_sync_offset(&data[offset..]) {
                offset += resync;
                continue;
            } else {
                break;
            }
        }

        packets_parsed += 1;

        // Header fields
        let tei = (packet[1] & 0x80) != 0; // Transport Error Indicator
        if tei {
            offset += TS_PACKET_SIZE;
            continue;
        }

        let pusi = (packet[1] & 0x40) != 0; // Payload Unit Start Indicator
        let pid = (((packet[1] & 0x1f) as u16) << 8) | (packet[2] as u16);
        let afc = (packet[3] & 0x30) >> 4; // Adaptation field control: 1=payload only, 2=adapt only, 3=both
        let _continuity_counter = packet[3] & 0x0f;

        let mut payload_start = 4usize;

        // Adaptation field parsing
        if afc == 2 || afc == 3 {
            let adapt_len = packet[4] as usize;
            if adapt_len + 5 > TS_PACKET_SIZE {
                return Err(ProbeError::malformed(
                    "ts",
                    "invalid adaptation field length",
                ));
            }
            if adapt_len > 0 {
                let flags = packet[5];
                let pcr_flag = (flags & 0x10) != 0;
                if pcr_flag && adapt_len >= 7 && initial_pcr.is_none() {
                    // PCR is 33-bit base (at bytes 6..10) + 6 reserved bits + 9-bit ext (bytes 10..12)
                    let pcr_base = ((packet[6] as u64) << 25)
                        | ((packet[7] as u64) << 17)
                        | ((packet[8] as u64) << 9)
                        | ((packet[9] as u64) << 1)
                        | ((packet[10] as u64) >> 7);
                    let pcr_ext = (((packet[10] & 0x01) as u64) << 8) | (packet[11] as u64);
                    let pcr_val = pcr_base * 300 + pcr_ext;
                    initial_pcr = Some(pcr_val);
                }
            }
            payload_start = 5 + adapt_len;
        }

        let has_payload = afc == 1 || afc == 3;
        if has_payload && payload_start < TS_PACKET_SIZE {
            let payload = &packet[payload_start..];
            if pid == 0x0000 {
                // PAT (Program Association Table)
                parse_pat(payload, pusi, &mut pmt_pids)?;
            } else if pmt_pids.contains(&pid) {
                // PMT (Program Map Table)
                parse_pmt(payload, pusi, &mut elementary_streams)?;
            }
        }

        offset += TS_PACKET_SIZE;
    }

    if packets_parsed == 0 {
        return Err(ProbeError::Truncated("ts"));
    }
    if pmt_pids.is_empty() || elementary_streams.is_empty() {
        return Err(ProbeError::Truncated("ts elementary streams"));
    }

    desc.pcr = initial_pcr;

    // Map discovered elementary streams to video/audio info
    for (stype, _epid) in elementary_streams {
        match stype {
            // Video stream types
            0x1b if desc.video.is_none() => {
                // H.264
                desc.video = Some(VideoInfo::new(VideoCodec::H264));
            }
            0x24 if desc.video.is_none() => {
                // HEVC (H.265)
                desc.video = Some(VideoInfo::new(VideoCodec::Hevc));
            }
            0x01 | 0x02 if desc.video.is_none() => {
                // MPEG-1 / MPEG-2 Video
                desc.video = Some(VideoInfo::new(VideoCodec::Mpeg2));
            }
            0x10 if desc.video.is_none() => {
                // MPEG-4 Part 2 Visual
                desc.video = Some(VideoInfo::new(VideoCodec::Mpeg4Part2));
            }
            // Audio stream types
            0x0f | 0x11 if desc.audio.is_none() => {
                // AAC (ADTS / LATM)
                desc.audio = Some(AudioInfo::new(AudioCodec::Aac));
            }
            0x03 | 0x04 if desc.audio.is_none() => {
                // MPEG-1 / MPEG-2 Audio (MP3 / layer 1/2)
                desc.audio = Some(AudioInfo::new(AudioCodec::Mp3));
            }
            _ => {}
        }
    }

    Ok(desc)
}

fn parse_pat(
    mut payload: &[u8],
    pusi: bool,
    pmt_pids: &mut HashSet<u16>,
) -> Result<(), ProbeError> {
    if pusi {
        if payload.is_empty() {
            return Err(ProbeError::Truncated("ts pat pointer"));
        }
        let pointer = payload[0] as usize;
        if pointer + 1 > payload.len() {
            return Err(ProbeError::Truncated("ts pat pointer"));
        }
        payload = &payload[pointer + 1..];
    }
    if payload.len() < 8 {
        return Ok(()); // Truncated or stuffing, ignore gracefully
    }
    let table_id = payload[0];
    if table_id != 0x00 {
        return Ok(()); // Not a PAT
    }
    let section_syntax_indicator = (payload[1] & 0x80) != 0;
    if !section_syntax_indicator {
        return Err(ProbeError::malformed("ts", "invalid PAT syntax indicator"));
    }
    let section_length = (((payload[1] & 0x0f) as usize) << 8) | (payload[2] as usize);
    if section_length < 9 || payload.len() < 3 + section_length {
        return Err(ProbeError::Truncated("ts pat section"));
    }

    // Skip transport_stream_id (2), version/cni (1), section_number (1), last_section_number (1) = 5 bytes
    // Data entries start at index 8, section ends before 4-byte CRC
    let entries_start = 8;
    let entries_end = 3 + section_length - 4; // exclude CRC32
    if entries_end < entries_start || entries_end > payload.len() {
        return Err(ProbeError::malformed("ts", "bad PAT section length"));
    }

    let mut pos = entries_start;
    while pos + 4 <= entries_end {
        let program_number = u16::from_be_bytes([payload[pos], payload[pos + 1]]);
        let pid = (((payload[pos + 2] & 0x1f) as u16) << 8) | (payload[pos + 3] as u16);
        if program_number != 0 {
            pmt_pids.insert(pid);
        }
        pos += 4;
    }
    Ok(())
}

fn parse_pmt(
    mut payload: &[u8],
    pusi: bool,
    elementary_streams: &mut Vec<(u8, u16)>,
) -> Result<(), ProbeError> {
    if pusi {
        if payload.is_empty() {
            return Err(ProbeError::Truncated("ts pmt pointer"));
        }
        let pointer = payload[0] as usize;
        if pointer + 1 > payload.len() {
            return Err(ProbeError::Truncated("ts pmt pointer"));
        }
        payload = &payload[pointer + 1..];
    }
    if payload.len() < 12 {
        return Ok(()); // Truncated or stuffing
    }
    let table_id = payload[0];
    if table_id != 0x02 {
        return Ok(()); // Not PMT table ID
    }
    let section_syntax_indicator = (payload[1] & 0x80) != 0;
    if !section_syntax_indicator {
        return Err(ProbeError::malformed("ts", "invalid PMT syntax indicator"));
    }
    let section_length = (((payload[1] & 0x0f) as usize) << 8) | (payload[2] as usize);
    if section_length < 13 || payload.len() < 3 + section_length {
        return Err(ProbeError::Truncated("ts pmt section"));
    }

    let program_info_length = (((payload[10] & 0x0f) as usize) << 8) | (payload[11] as usize);
    let mut pos = 12 + program_info_length;
    let section_end = 3 + section_length - 4; // exclude CRC32
    if section_end < pos || section_end > payload.len() {
        return Err(ProbeError::malformed("ts", "bad PMT section length"));
    }

    while pos + 5 <= section_end {
        let stream_type = payload[pos];
        let elementary_pid = (((payload[pos + 1] & 0x1f) as u16) << 8) | (payload[pos + 2] as u16);
        let es_info_length =
            (((payload[pos + 3] & 0x0f) as usize) << 8) | (payload[pos + 4] as usize);
        elementary_streams.push((stream_type, elementary_pid));
        pos += 5 + es_info_length;
    }

    Ok(())
}
