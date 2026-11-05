//! Minimal pure-Rust MPEG-TS muxer: PAT, PMT, PES, and PCR.
//!
//! Generates standard 188-byte MPEG-2 Transport Stream packets compliant with
//! ISO/IEC 13818-1, compatible with HLS and `hls.js`.

use mediamaster_frame::{CodecFormat, FramePacket, StreamKind};

/// Standard MPEG-TS packet size in bytes.
pub const TS_PACKET_SIZE: usize = 188;
/// TS sync byte `0x47`.
pub const TS_SYNC_BYTE: u8 = 0x47;

/// PID allocations
pub const PID_PAT: u16 = 0x0000;
pub const PID_PMT: u16 = 0x1000;
pub const PID_PCR: u16 = 0x0100;
pub const PID_VIDEO: u16 = 0x0100;
pub const PID_AUDIO: u16 = 0x0101;

/// Stream types in PMT
pub const STREAM_TYPE_H264: u8 = 0x1B;
pub const STREAM_TYPE_H265: u8 = 0x24;
pub const STREAM_TYPE_AAC: u8 = 0x0F;

/// MPEG-2 CRC-32 table/computation for PAT and PMT sections.
pub fn mpeg_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        for bit in 0..8 {
            let bit_val = ((byte >> (7 - bit)) & 1) as u32;
            let c31 = (crc >> 31) & 1;
            crc <<= 1;
            if (c31 ^ bit_val) != 0 {
                crc ^= 0x04C11DB7;
            }
        }
    }
    crc
}

/// Minimal MPEG-TS muxer.
#[derive(Debug, Default)]
pub struct TsMuxer {
    pat_continuity: u8,
    pmt_continuity: u8,
    video_continuity: u8,
    audio_continuity: u8,
}

impl TsMuxer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset continuity counters (for fresh segment generation if desired).
    pub fn reset_counters(&mut self) {
        self.pat_continuity = 0;
        self.pmt_continuity = 0;
        self.video_continuity = 0;
        self.audio_continuity = 0;
    }

    /// Build a PAT (Program Association Table) 188-byte TS packet.
    pub fn build_pat(&mut self) -> [u8; TS_PACKET_SIZE] {
        let mut pkt = [0xFFu8; TS_PACKET_SIZE];
        pkt[0] = TS_SYNC_BYTE;
        // Transport error(0) | Payload unit start(1) | Transport priority(0) | PID (0x0000)
        pkt[1] = 0x40 | ((PID_PAT >> 8) as u8 & 0x1F);
        pkt[2] = (PID_PAT & 0xFF) as u8;
        // Scrambling(00) | Adaptation field control(01 - payload only) | Continuity counter
        pkt[3] = 0x10 | (self.pat_continuity & 0x0F);
        self.pat_continuity = (self.pat_continuity + 1) & 0x0F;

        // Pointer field: 0
        pkt[4] = 0x00;

        // PAT section data
        // Table ID: 0x00
        // Section syntax indicator(1) | '0'(0) | Reserved(11) | Section length(12-bit)
        // Section length = 5 (syntax fields) + 4 (program 1) + 4 (CRC32) = 13 bytes (0x00D)
        let section_len: u16 = 13;
        let mut section = Vec::with_capacity(section_len as usize + 3);
        section.push(0x00); // table_id
        section.push(0xB0 | ((section_len >> 8) as u8 & 0x0F));
        section.push((section_len & 0xFF) as u8);
        section.push(0x00); // transport_stream_id high
        section.push(0x01); // transport_stream_id low
        section.push(0xC1); // version_number(0), current_next_indicator(1)
        section.push(0x00); // section_number
        section.push(0x00); // last_section_number

        // Program 1: program_number = 1, PMT PID = PID_PMT
        section.push(0x00);
        section.push(0x01);
        section.push(0xE0 | ((PID_PMT >> 8) as u8 & 0x1F));
        section.push((PID_PMT & 0xFF) as u8);

        // CRC32
        let crc = mpeg_crc32(&section);
        section.extend_from_slice(&crc.to_be_bytes());

        // Copy section into packet payload starting at offset 5
        pkt[5..5 + section.len()].copy_from_slice(&section);
        pkt
    }

    /// Build a PMT (Program Map Table) 188-byte TS packet.
    pub fn build_pmt(
        &mut self,
        has_video: bool,
        video_codec: Option<CodecFormat>,
        has_audio: bool,
    ) -> [u8; TS_PACKET_SIZE] {
        let mut pkt = [0xFFu8; TS_PACKET_SIZE];
        pkt[0] = TS_SYNC_BYTE;
        pkt[1] = 0x40 | ((PID_PMT >> 8) as u8 & 0x1F);
        pkt[2] = (PID_PMT & 0xFF) as u8;
        pkt[3] = 0x10 | (self.pmt_continuity & 0x0F);
        self.pmt_continuity = (self.pmt_continuity + 1) & 0x0F;

        // Pointer field: 0
        pkt[4] = 0x00;

        // PMT section
        let mut streams = Vec::new();
        if has_video {
            let st = match video_codec {
                Some(CodecFormat::HevcAnnexB) => STREAM_TYPE_H265,
                _ => STREAM_TYPE_H264,
            };
            streams.push(st);
            streams.push(0xE0 | ((PID_VIDEO >> 8) as u8 & 0x1F));
            streams.push((PID_VIDEO & 0xFF) as u8);
            streams.push(0xF0); // ES info length = 0
            streams.push(0x00);
        }
        if has_audio {
            streams.push(STREAM_TYPE_AAC);
            streams.push(0xE0 | ((PID_AUDIO >> 8) as u8 & 0x1F));
            streams.push((PID_AUDIO & 0xFF) as u8);
            streams.push(0xF0); // ES info length = 0
            streams.push(0x00);
        }

        // Section length = 9 (syntax fields) + streams.len() + 4 (CRC32)
        let section_len: u16 = 9 + streams.len() as u16 + 4;
        let mut section = Vec::with_capacity(section_len as usize + 3);
        section.push(0x02); // table_id = 0x02 (PMT)
        section.push(0xB0 | ((section_len >> 8) as u8 & 0x0F));
        section.push((section_len & 0xFF) as u8);
        section.push(0x00); // program_number high
        section.push(0x01); // program_number low
        section.push(0xC1); // version 0, current
        section.push(0x00); // section_number
        section.push(0x00); // last_section_number

        // PCR PID = PID_PCR
        section.push(0xE0 | ((PID_PCR >> 8) as u8 & 0x1F));
        section.push((PID_PCR & 0xFF) as u8);

        // Program info length = 0
        section.push(0xF0);
        section.push(0x00);

        // Elementary streams
        section.extend_from_slice(&streams);

        let crc = mpeg_crc32(&section);
        section.extend_from_slice(&crc.to_be_bytes());

        pkt[5..5 + section.len()].copy_from_slice(&section);
        pkt
    }

    /// Build a PES (Packetized Elementary Stream) packet for a given `FramePacket`.
    pub fn build_pes_packet(packet: &FramePacket) -> Vec<u8> {
        let stream_id = match packet.kind {
            StreamKind::Video => 0xE0, // Video stream 0
            StreamKind::Audio => 0xC0, // Audio stream 0
        };

        // Convert timestamps to 90 kHz MPEG timebase
        let pts_90k = if packet.timescale == 90_000 {
            packet.pts
        } else if packet.timescale > 0 {
            (packet.pts as i128 * 90_000 / packet.timescale as i128) as i64
        } else {
            0
        };

        let dts_90k = if packet.timescale == 90_000 {
            packet.dts
        } else if packet.timescale > 0 {
            (packet.dts as i128 * 90_000 / packet.timescale as i128) as i64
        } else {
            0
        };

        let has_dts = packet.kind == StreamKind::Video && pts_90k != dts_90k;

        // Flags
        // byte 7: '10' (reserved), scrambling(00), priority(0), data_alignment(1), copyright(0), original(1)
        let flags1 = 0x85;
        // byte 8: pts_dts_flags (2 bits): '10' for PTS only, '11' for PTS+DTS
        let pts_dts_flags = if has_dts { 0b11 } else { 0b10 };
        let flags2 = pts_dts_flags << 6;

        let pes_header_data_len: u8 = if has_dts { 10 } else { 5 };

        let payload_len = packet.data.len();
        // PES packet length: if video and payload_len > 65535 - 3 - pes_header_data_len, length can be 0.
        let total_pes_len = 3 + pes_header_data_len as usize + payload_len;
        let pes_packet_length = if packet.kind == StreamKind::Video && total_pes_len > 0xFFFF {
            0u16
        } else {
            total_pes_len.min(0xFFFF) as u16
        };

        let mut pes = Vec::with_capacity(6 + 3 + pes_header_data_len as usize + payload_len);
        // Start code prefix 0x000001 + stream_id
        pes.extend_from_slice(&[0x00, 0x00, 0x01, stream_id]);
        pes.extend_from_slice(&pes_packet_length.to_be_bytes());
        pes.push(flags1);
        pes.push(flags2);
        pes.push(pes_header_data_len);

        // Encode PTS (and DTS if present)
        encode_ts_timestamp(&mut pes, pts_dts_flags, pts_90k);
        if has_dts {
            encode_ts_timestamp(&mut pes, 0b0001, dts_90k);
        }

        pes.extend_from_slice(&packet.data);
        pes
    }

    /// Packetize a PES byte vector into 188-byte MPEG-TS packets.
    pub fn mux_pes(&mut self, pid: u16, pes_data: &[u8], pcr_val: Option<u64>) -> Vec<u8> {
        let mut out = Vec::with_capacity((pes_data.len() / 184 + 2) * TS_PACKET_SIZE);
        let mut offset = 0;
        let mut first = true;

        while offset < pes_data.len() {
            let mut pkt = [0xFFu8; TS_PACKET_SIZE];
            pkt[0] = TS_SYNC_BYTE;

            let continuity = match pid {
                PID_VIDEO => {
                    let c = self.video_continuity;
                    self.video_continuity = (self.video_continuity + 1) & 0x0F;
                    c
                }
                PID_AUDIO => {
                    let c = self.audio_continuity;
                    self.audio_continuity = (self.audio_continuity + 1) & 0x0F;
                    c
                }
                _ => 0,
            };

            let payload_unit_start = if first { 0x40 } else { 0x00 };
            pkt[1] = payload_unit_start | ((pid >> 8) as u8 & 0x1F);
            pkt[2] = (pid & 0xFF) as u8;

            let remaining_pes = pes_data.len() - offset;

            if first && pcr_val.is_some() {
                // Header with adaptation field carrying PCR
                if let Some(pcr) = pcr_val {
                    let adapt_len = 7; // 1 flag byte + 6 PCR bytes
                    pkt[3] = 0x30 | continuity; // payload + adaptation
                    pkt[4] = adapt_len;
                    pkt[5] = 0x10; // PCR flag set
                    write_pcr(&mut pkt[6..12], pcr);

                    let header_overhead = 4 + 1 + adapt_len as usize;
                    let capacity = TS_PACKET_SIZE - header_overhead;
                    let chunk_size = remaining_pes.min(capacity);

                    if chunk_size < capacity {
                        // Need stuffing in adaptation field
                        let extra_stuffing = capacity - chunk_size;
                        let total_adapt_len = adapt_len + extra_stuffing as u8;
                        pkt[4] = total_adapt_len;
                        // Move PCR
                        write_pcr(&mut pkt[6..12], pcr);
                        // Fill remaining adaptation with 0xFF
                        pkt[12..12 + extra_stuffing].fill(0xFF);
                        let payload_start = 5 + total_adapt_len as usize;
                        pkt[payload_start..payload_start + chunk_size]
                            .copy_from_slice(&pes_data[offset..offset + chunk_size]);
                    } else {
                        pkt[header_overhead..header_overhead + chunk_size]
                            .copy_from_slice(&pes_data[offset..offset + chunk_size]);
                    }
                    offset += chunk_size;
                }
            } else if remaining_pes < 184 {
                // Last packet requiring adaptation stuffing
                pkt[3] = 0x30 | continuity; // payload + adaptation
                let stuffing = 184 - remaining_pes;
                pkt[4] = (stuffing - 1) as u8;
                if stuffing > 1 {
                    pkt[5] = 0x00; // flags
                    pkt[6..4 + stuffing].fill(0xFF);
                }
                let payload_start = 4 + stuffing;
                pkt[payload_start..payload_start + remaining_pes]
                    .copy_from_slice(&pes_data[offset..]);
                offset += remaining_pes;
            } else {
                // Normal 184 byte payload packet
                pkt[3] = 0x10 | continuity; // payload only
                pkt[4..188].copy_from_slice(&pes_data[offset..offset + 184]);
                offset += 184;
            }

            first = false;
            out.extend_from_slice(&pkt);
        }

        out
    }

    /// Mux a complete segment consisting of PAT, PMT, and consecutive video/audio packets.
    pub fn mux_segment(&mut self, packets: &[FramePacket]) -> Vec<u8> {
        let has_video = packets.iter().any(|p| p.kind == StreamKind::Video);
        let has_audio = packets.iter().any(|p| p.kind == StreamKind::Audio);
        let video_codec = packets
            .iter()
            .find(|p| p.kind == StreamKind::Video)
            .map(|p| p.codec);

        let mut seg = Vec::new();
        // Emits PAT and PMT
        seg.extend_from_slice(&self.build_pat());
        seg.extend_from_slice(&self.build_pmt(has_video, video_codec, has_audio));

        for pkt in packets {
            let pid = match pkt.kind {
                StreamKind::Video => PID_VIDEO,
                StreamKind::Audio => PID_AUDIO,
            };
            let pcr = if pkt.kind == StreamKind::Video && pkt.is_keyframe {
                let pts_90k = if pkt.timescale == 90_000 {
                    pkt.pts
                } else if pkt.timescale > 0 {
                    (pkt.pts as i128 * 90_000 / pkt.timescale as i128) as i64
                } else {
                    0
                };
                Some((pts_90k.max(0) as u64) * 300)
            } else {
                None
            };

            let pes = Self::build_pes_packet(pkt);
            let ts_pkts = self.mux_pes(pid, &pes, pcr);
            seg.extend_from_slice(&ts_pkts);
        }

        seg
    }
}

fn write_pcr(buf: &mut [u8], pcr_27mhz: u64) {
    let pcr_base = (pcr_27mhz / 300) & 0x1FFFFFFFF;
    let pcr_ext = (pcr_27mhz % 300) as u16;

    buf[0] = (pcr_base >> 25) as u8;
    buf[1] = (pcr_base >> 17) as u8;
    buf[2] = (pcr_base >> 9) as u8;
    buf[3] = (pcr_base >> 1) as u8;
    buf[4] = (((pcr_base & 1) << 7) | 0x7E | ((pcr_ext >> 8) & 1) as u64) as u8;
    buf[5] = (pcr_ext & 0xFF) as u8;
}

fn encode_ts_timestamp(out: &mut Vec<u8>, prefix_bits: u8, ts_90k: i64) {
    let ts = (ts_90k.max(0) as u64) & 0x1FFFFFFFF; // 33-bit timestamp
    let b0 = ((prefix_bits & 0x0F) << 4) | (((ts >> 30) as u8 & 0x07) << 1) | 1;
    let b1 = (ts >> 22) as u8;
    let b2 = (((ts >> 15) as u8 & 0x7F) << 1) | 1;
    let b3 = (ts >> 7) as u8;
    let b4 = (((ts & 0x7F) as u8) << 1) | 1;
    out.extend_from_slice(&[b0, b1, b2, b3, b4]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pat_and_pmt_generate_188_byte_packets() {
        let mut muxer = TsMuxer::new();
        let pat = muxer.build_pat();
        assert_eq!(pat.len(), 188);
        assert_eq!(pat[0], TS_SYNC_BYTE);

        let pmt = muxer.build_pmt(true, Some(CodecFormat::H264AnnexB), true);
        assert_eq!(pmt.len(), 188);
        assert_eq!(pmt[0], TS_SYNC_BYTE);
    }

    #[test]
    fn pes_muxing_packetizes_cleanly() {
        let mut muxer = TsMuxer::new();
        let pkt = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            90_000,
            90_000,
            90_000,
            3000,
            true,
            vec![0, 0, 0, 1, 0x65, 0x88, 0x84],
        );
        let pes = TsMuxer::build_pes_packet(&pkt);
        let ts = muxer.mux_pes(PID_VIDEO, &pes, Some(90_000 * 300));
        assert!(!ts.is_empty());
        assert_eq!(ts.len() % 188, 0);
        assert_eq!(ts[0], TS_SYNC_BYTE);
    }
}
