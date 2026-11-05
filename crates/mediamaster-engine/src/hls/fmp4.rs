//! fMP4 CMAF init and media segment generator.
//!
//! Generates CMAF-compliant fragmented MP4 (`ftyp` + `moov` init segment,
//! and `moof` + `mdat` media segments) for HLS with AV1/H264/HEVC/AAC.

use mediamaster_frame::{CodecFormat, FramePacket};

fn boxed(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = (8 + payload.len()) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(fourcc);
    out.extend_from_slice(payload);
    out
}

/// fMP4 CMAF muxer for generating init segments and media segments.
pub struct Fmp4Muxer {
    track_id: u32,
    timescale: u32,
    sequence_number: u32,
}

impl Fmp4Muxer {
    pub fn new(track_id: u32, timescale: u32) -> Self {
        Self {
            track_id,
            timescale,
            sequence_number: 1,
        }
    }

    /// Reset sequence number.
    pub fn set_sequence_number(&mut self, seq: u32) {
        self.sequence_number = seq;
    }

    /// Generate an init segment (`ftyp` + `moov`).
    pub fn build_init_segment(&self, codec: CodecFormat, width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();

        // ftyp: major_brand = cmfc, minor_version = 0, compatible_brands = cmfc, isom, mp42
        let mut ftyp_payload = Vec::new();
        ftyp_payload.extend_from_slice(b"cmfc");
        ftyp_payload.extend_from_slice(&0u32.to_be_bytes());
        ftyp_payload.extend_from_slice(b"cmfcmp42isom");
        out.extend_from_slice(&boxed(b"ftyp", &ftyp_payload));

        // moov: mvhd + trak + mvex
        // mvhd (version 0)
        let mut mvhd = vec![0u8; 100];
        mvhd[12..16].copy_from_slice(&self.timescale.to_be_bytes());
        mvhd[16..20].copy_from_slice(&0u32.to_be_bytes()); // duration = 0 in fragmented
        mvhd[96..100].copy_from_slice(&2u32.to_be_bytes()); // next_track_id = 2
        let mvhd_box = boxed(b"mvhd", &mvhd);

        // trak: tkhd + mdia
        let mut tkhd = vec![0u8; 84];
        tkhd[3] = 0x07; // flags: enabled, in_movie, in_preview
        tkhd[12..16].copy_from_slice(&self.track_id.to_be_bytes());
        tkhd[76..80].copy_from_slice(&(width << 16).to_be_bytes());
        tkhd[80..84].copy_from_slice(&(height << 16).to_be_bytes());
        let tkhd_box = boxed(b"tkhd", &tkhd);

        // mdhd
        let mut mdhd = vec![0u8; 24];
        mdhd[12..16].copy_from_slice(&self.timescale.to_be_bytes());
        let mdhd_box = boxed(b"mdhd", &mdhd);

        // hdlr
        let mut hdlr = vec![0u8; 24];
        hdlr[8..12].copy_from_slice(b"vide");
        let hdlr_box = boxed(b"hdlr", &hdlr);

        // minf: vmhd + dinf + stbl
        let vmhd_box = boxed(b"vmhd", &[0, 0, 0, 1, 0, 0, 0, 0]);
        let dref = boxed(
            b"dref",
            &[
                0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 12, b'u', b'r', b'l', b' ', 0, 0, 0, 1,
            ],
        );
        let dinf_box = boxed(b"dinf", &dref);

        // stsd: sample entry
        let sample_entry = match codec {
            CodecFormat::H264AnnexB => {
                let mut e = vec![0u8; 78];
                e[6..8].copy_from_slice(&1u16.to_be_bytes()); // data_ref_idx
                e[24..26].copy_from_slice(&(width as u16).to_be_bytes());
                e[26..28].copy_from_slice(&(height as u16).to_be_bytes());
                e[74..76].copy_from_slice(&0x0018u16.to_be_bytes()); // depth 24
                e[76..78].copy_from_slice(&0xFFFFu16.to_be_bytes());
                let avcc = boxed(b"avcC", &[1, 0x64, 0, 0x29, 0xFF, 0xE1, 0, 0]);
                let mut v = e;
                v.extend_from_slice(&avcc);
                boxed(b"avc1", &v)
            }
            CodecFormat::HevcAnnexB => {
                let mut e = vec![0u8; 78];
                e[6..8].copy_from_slice(&1u16.to_be_bytes());
                e[24..26].copy_from_slice(&(width as u16).to_be_bytes());
                e[26..28].copy_from_slice(&(height as u16).to_be_bytes());
                let hvcc = boxed(b"hvcC", &[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x5D]);
                let mut v = e;
                v.extend_from_slice(&hvcc);
                boxed(b"hvc1", &v)
            }
            _ => {
                let mut e = vec![0u8; 28];
                e[6..8].copy_from_slice(&1u16.to_be_bytes());
                e[16..18].copy_from_slice(&2u16.to_be_bytes()); // 2 channels
                e[24..28].copy_from_slice(&(self.timescale << 16).to_be_bytes());
                let esds = boxed(
                    b"esds",
                    &[
                        0, 0, 0, 0, 3, 23, 0, 1, 0, 4, 15, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 5, 2,
                        0x12, 0x10, 6, 1, 2,
                    ],
                );
                let mut v = e;
                v.extend_from_slice(&esds);
                boxed(b"mp4a", &v)
            }
        };

        let mut stsd_payload = vec![0u8; 8];
        stsd_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        stsd_payload.extend_from_slice(&sample_entry);
        let stsd_box = boxed(b"stsd", &stsd_payload);

        let stts_box = boxed(b"stts", &[0; 8]);
        let stsc_box = boxed(b"stsc", &[0; 8]);
        let stsz_box = boxed(b"stsz", &[0; 12]);
        let stco_box = boxed(b"stco", &[0; 8]);

        let mut stbl_payload = Vec::new();
        stbl_payload.extend_from_slice(&stsd_box);
        stbl_payload.extend_from_slice(&stts_box);
        stbl_payload.extend_from_slice(&stsc_box);
        stbl_payload.extend_from_slice(&stsz_box);
        stbl_payload.extend_from_slice(&stco_box);
        let stbl_box = boxed(b"stbl", &stbl_payload);

        let mut minf_payload = Vec::new();
        minf_payload.extend_from_slice(&vmhd_box);
        minf_payload.extend_from_slice(&dinf_box);
        minf_payload.extend_from_slice(&stbl_box);
        let minf_box = boxed(b"minf", &minf_payload);

        let mut mdia_payload = Vec::new();
        mdia_payload.extend_from_slice(&mdhd_box);
        mdia_payload.extend_from_slice(&hdlr_box);
        mdia_payload.extend_from_slice(&minf_box);
        let mdia_box = boxed(b"mdia", &mdia_payload);

        let mut trak_payload = Vec::new();
        trak_payload.extend_from_slice(&tkhd_box);
        trak_payload.extend_from_slice(&mdia_box);
        let trak_box = boxed(b"trak", &trak_payload);

        // mvex: trex
        let mut trex = vec![0u8; 24];
        trex[4..8].copy_from_slice(&self.track_id.to_be_bytes());
        trex[8..12].copy_from_slice(&1u32.to_be_bytes()); // default_sample_description_index = 1
        let trex_box = boxed(b"trex", &trex);
        let mvex_box = boxed(b"mvex", &trex_box);

        let mut moov_payload = Vec::new();
        moov_payload.extend_from_slice(&mvhd_box);
        moov_payload.extend_from_slice(&trak_box);
        moov_payload.extend_from_slice(&mvex_box);
        out.extend_from_slice(&boxed(b"moov", &moov_payload));

        out
    }

    /// Build a media segment (`moof` + `mdat`) containing the given packets.
    pub fn build_media_segment(&mut self, packets: &[FramePacket]) -> Vec<u8> {
        if packets.is_empty() {
            return Vec::new();
        }

        let seq = self.sequence_number;
        self.sequence_number += 1;

        // mfhd: sequence_number
        let mut mfhd_payload = vec![0u8; 8];
        mfhd_payload[4..8].copy_from_slice(&seq.to_be_bytes());
        let mfhd_box = boxed(b"mfhd", &mfhd_payload);

        // tfhd: flags = default-base-is-moof (0x020000)
        let mut tfhd_payload = vec![0u8; 8];
        tfhd_payload[0..4].copy_from_slice(&0x020000u32.to_be_bytes());
        tfhd_payload[4..8].copy_from_slice(&self.track_id.to_be_bytes());
        let tfhd_box = boxed(b"tfhd", &tfhd_payload);

        // tfdt (v1, 64-bit baseMediaDecodeTime)
        let base_dts = packets[0].dts.max(0) as u64;
        let mut tfdt_payload = vec![0u8; 12];
        tfdt_payload[0] = 1; // version 1
        tfdt_payload[4..12].copy_from_slice(&base_dts.to_be_bytes());
        let tfdt_box = boxed(b"tfdt", &tfdt_payload);

        // Build sample data for mdat and entries for trun
        let mut mdat_payload = Vec::new();
        // trun: flags: data-offset-present(0x0001) | sample-duration-present(0x0100) |
        // sample-size-present(0x0200) | sample-flags-present(0x0400) | sample-composition-time-offsets-present(0x0800)
        let flags: u32 = 0x0001 | 0x0100 | 0x0200 | 0x0400 | 0x0800;
        let mut trun_payload = Vec::new();
        trun_payload.extend_from_slice(&flags.to_be_bytes());
        trun_payload.extend_from_slice(&(packets.len() as u32).to_be_bytes());
        // Placeholder for data_offset (will be fixed up)
        trun_payload.extend_from_slice(&0u32.to_be_bytes());

        for pkt in packets {
            // Annex-B to AVCC if NAL units (replace 0x000001 with 4-byte length)
            let nalu_data = to_length_delimited(&pkt.data);
            let sample_size = nalu_data.len() as u32;
            let sample_duration = pkt.duration as u32;
            let sample_flags: u32 = if pkt.is_keyframe {
                0x02000000 // depends_on=2 (I-frame)
            } else {
                0x01010000 // depends_on=1 (non-keyframe)
            };
            let comp_offset = (pkt.pts - pkt.dts) as i32;

            trun_payload.extend_from_slice(&sample_duration.to_be_bytes());
            trun_payload.extend_from_slice(&sample_size.to_be_bytes());
            trun_payload.extend_from_slice(&sample_flags.to_be_bytes());
            trun_payload.extend_from_slice(&(comp_offset as u32).to_be_bytes());

            mdat_payload.extend_from_slice(&nalu_data);
        }

        // Calculate moof total size to fix up data_offset in trun
        // moof header: 8 bytes
        // mfhd: mfhd_box.len()
        // traf header: 8 bytes
        // tfhd: tfhd_box.len()
        // tfdt: tfdt_box.len()
        // trun: 8 + trun_payload.len()
        let trun_box_len = 8 + trun_payload.len();
        let traf_payload_len = tfhd_box.len() + tfdt_box.len() + trun_box_len;
        let traf_box_len = 8 + traf_payload_len;
        let moof_box_len = 8 + mfhd_box.len() + traf_box_len;
        let data_offset = (moof_box_len + 8) as u32; // +8 for mdat box header

        // Write actual data_offset into trun_payload (at offset 8..12)
        trun_payload[8..12].copy_from_slice(&data_offset.to_be_bytes());
        let trun_box = boxed(b"trun", &trun_payload);

        let mut traf_payload = Vec::new();
        traf_payload.extend_from_slice(&tfhd_box);
        traf_payload.extend_from_slice(&tfdt_box);
        traf_payload.extend_from_slice(&trun_box);
        let traf_box = boxed(b"traf", &traf_payload);

        let mut moof_payload = Vec::new();
        moof_payload.extend_from_slice(&mfhd_box);
        moof_payload.extend_from_slice(&traf_box);
        let moof_box = boxed(b"moof", &moof_payload);

        let mdat_box = boxed(b"mdat", &mdat_payload);

        let mut seg = Vec::with_capacity(moof_box.len() + mdat_box.len());
        seg.extend_from_slice(&moof_box);
        seg.extend_from_slice(&mdat_box);
        seg
    }
}

/// Convert Annex-B NAL units to length-delimited format (4-byte big endian length).
fn to_length_delimited(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut i = 0;
    while i < data.len() {
        if i + 3 <= data.len() && data[i..i + 3] == [0, 0, 1] {
            i += 3;
        } else if i + 4 <= data.len() && data[i..i + 4] == [0, 0, 0, 1] {
            i += 4;
        } else {
            // If already length delimited or plain payload
            return data.to_vec();
        }

        // Find next NAL start
        let start = i;
        while i < data.len() {
            if (i + 3 <= data.len() && data[i..i + 3] == [0, 0, 1])
                || (i + 4 <= data.len() && data[i..i + 4] == [0, 0, 0, 1])
            {
                break;
            }
            i += 1;
        }
        let nal_len = (i - start) as u32;
        out.extend_from_slice(&nal_len.to_be_bytes());
        out.extend_from_slice(&data[start..i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_segment_starts_with_ftyp_and_has_moov() {
        let muxer = Fmp4Muxer::new(1, 90_000);
        let init = muxer.build_init_segment(CodecFormat::H264AnnexB, 1920, 1080);
        assert!(init.len() > 16);
        assert_eq!(&init[4..8], b"ftyp");
        assert!(init.windows(4).any(|w| w == b"moov"));
    }

    #[test]
    fn media_segment_starts_with_moof_and_has_mdat() {
        let mut muxer = Fmp4Muxer::new(1, 90_000);
        let pkt = FramePacket::new_video(
            1,
            CodecFormat::H264AnnexB,
            0,
            0,
            90_000,
            3000,
            true,
            vec![0, 0, 0, 1, 0x65, 0x88, 0x84],
        );
        let media = muxer.build_media_segment(&[pkt]);
        assert_eq!(&media[4..8], b"moof");
        assert!(media.windows(4).any(|w| w == b"mdat"));
    }
}
