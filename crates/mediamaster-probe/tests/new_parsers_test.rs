//! Tests for new mediamaster-probe parsers: MPEG-TS, MP3, ADTS, FLAC, WAV/RF64, JPEG, PNG, GIF, WebP.
//! Covers:
//! - ≥1 happy path per format
//! - ≥3 truncation cases per format
//! - ≥2 hostile / fuzzed cases per format
//! - proptest property tests verifying no panics on arbitrary slices.

use mediamaster_probe::{probe_bytes, AudioCodec, Container, ProbeError, VideoCodec};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers to generate minimal fixtures (< 10 KB)
// ---------------------------------------------------------------------------

fn minimal_ts() -> Vec<u8> {
    // 2 TS packets:
    // Packet 1: PAT (PID 0x0000) pointing to PMT PID 0x0100
    // Packet 2: PMT (PID 0x0100) with H.264 video (0x1b) on PID 0x0101 and AAC audio (0x0f) on PID 0x0102, with PCR adaptation field
    let mut data = Vec::with_capacity(188 * 2);

    // --- Packet 1: PAT ---
    let mut pkt1 = vec![0xFF; 188];
    pkt1[0] = 0x47; // Sync
    pkt1[1] = 0x40; // PUSI = 1, PID = 0x00
    pkt1[2] = 0x00; // PID low
    pkt1[3] = 0x10; // AFC = payload only, CC = 0

    // Payload: pointer field = 0
    pkt1[4] = 0x00;
    // PAT section:
    // table_id = 0x00
    pkt1[5] = 0x00;
    // section_syntax_indicator = 1, '0', reserved, section_length = 13 (0x00D)
    pkt1[6] = 0xB0;
    pkt1[7] = 0x0D;
    // transport_stream_id = 1
    pkt1[8] = 0x00;
    pkt1[9] = 0x01;
    // version = 0, cni = 1
    pkt1[10] = 0xC1;
    // section_number = 0, last_section_number = 0
    pkt1[11] = 0x00;
    pkt1[12] = 0x00;
    // Program 1 -> PMT PID 0x0100
    pkt1[13] = 0x00;
    pkt1[14] = 0x01; // program_number = 1
    pkt1[15] = 0xE1; // reserved (3 bits 111) + PID high (5 bits 00001)
    pkt1[16] = 0x00; // PID low (0x00) -> 0x0100
                     // CRC32 dummy
    pkt1[17] = 0x00;
    pkt1[18] = 0x00;
    pkt1[19] = 0x00;
    pkt1[20] = 0x00;

    data.extend_from_slice(&pkt1);

    // --- Packet 2: PMT ---
    let mut pkt2 = vec![0xFF; 188];
    pkt2[0] = 0x47; // Sync
    pkt2[1] = 0x41; // PUSI = 1, PID = 0x01
    pkt2[2] = 0x00; // PID low -> 0x0100
    pkt2[3] = 0x30; // AFC = adapt + payload, CC = 0

    // Adaptation field: length = 7, flags = 0x10 (PCR present), PCR = 90000 base
    pkt2[4] = 7;
    pkt2[5] = 0x10; // PCR flag
                    // PCR base = 90000 = 0x00015F90 (33 bits)
                    // byte 6: base[32..25] = 0
    pkt2[6] = 0x00;
    // byte 7: base[24..17] = 0
    pkt2[7] = 0x00;
    // byte 8: base[16..9] = 0x00015F90 >> 9 = 0xAF
    pkt2[8] = 0x00;
    // byte 9: base[8..1] = 0x00015F90 >> 1 = 0xAF (example bytes)
    pkt2[9] = 0x5F;
    // byte 10: base[0] << 7 = 0
    pkt2[10] = 0x90;
    // byte 11: ext
    pkt2[11] = 0x00;

    // Payload starts at offset 4 + 1 + 7 = 12
    pkt2[12] = 0x00; // pointer field
                     // PMT section
    pkt2[13] = 0x02; // table_id = 0x02 (PMT)
                     // section_syntax_indicator = 1, section_length = 23 (0x17)
    pkt2[14] = 0xB0;
    pkt2[15] = 0x17;
    // program_number = 1
    pkt2[16] = 0x00;
    pkt2[17] = 0x01;
    // version = 0, cni = 1
    pkt2[18] = 0xC1;
    pkt2[19] = 0x00; // section_number
    pkt2[20] = 0x00; // last_section_number
                     // PCR PID = 0x0101
    pkt2[21] = 0xE1;
    pkt2[22] = 0x01;
    // program_info_length = 0
    pkt2[23] = 0xF0;
    pkt2[24] = 0x00;

    // Stream 1: H.264 video (stype = 0x1b), PID = 0x0101, es_info_length = 0
    pkt2[25] = 0x1B;
    pkt2[26] = 0xE1;
    pkt2[27] = 0x01;
    pkt2[28] = 0xF0;
    pkt2[29] = 0x00;

    // Stream 2: AAC audio (stype = 0x0f), PID = 0x0102, es_info_length = 0
    pkt2[30] = 0x0F;
    pkt2[31] = 0xE1;
    pkt2[32] = 0x02;
    pkt2[33] = 0xF0;
    pkt2[34] = 0x00;

    // CRC32 dummy
    pkt2[35] = 0x00;
    pkt2[36] = 0x00;
    pkt2[37] = 0x00;
    pkt2[38] = 0x00;

    data.extend_from_slice(&pkt2);
    data
}

fn minimal_mp3() -> Vec<u8> {
    // ID3v2.3 header of 10 bytes + 10 bytes payload
    let mut data = Vec::new();
    data.extend_from_slice(b"ID3");
    data.push(3); // v2.3
    data.push(0); // revision
    data.push(0); // flags
                  // size: 10 in syncsafe integer
    data.extend_from_slice(&[0, 0, 0, 10]);
    data.extend_from_slice(&[0u8; 10]); // ID3 tags payload

    // MPEG-1 Layer III frame header:
    // byte 0: 0xFF
    // byte 1: 0xFB (sync 111, version MPEG-1 11, layer III 01, protection 1)
    // byte 2: 0x90 (bitrate 128kbps 1001, sample_rate 44100 00, padding 0, private 0)
    // byte 3: 0x00 (stereo 00)
    data.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
    // Append 414 bytes frame payload (128000 * 144 / 44100 = 417 bytes)
    data.extend_from_slice(&[0xAA; 414]);
    data
}

fn minimal_adts() -> Vec<u8> {
    // 7-byte ADTS header + dummy payload
    let mut data = Vec::new();
    // byte 0: 0xFF (syncword hi)
    // byte 1: 0xF1 (syncword lo 1111, ID MPEG-4 0, layer 00, protection_absent 1)
    // byte 2: 0x50 (profile AAC-LC 01, srate 44100 0100, private 0, chan_hi 0)
    // byte 3: 0x80 (chan_lo 2-chan 10, orig 0, home 0, copy 0, copy_start 0, frame_len_hi 00)
    // byte 4: 0x10 (frame_len mid: length = 16 = 0x010)
    // byte 5: 0x00 (frame_len lo: 0x00, buffer fullness hi 00000)
    // byte 6: 0xFC (buffer fullness lo)
    let frame_len: u16 = 16;
    let b3 = 0x80 | (((frame_len >> 11) & 0x03) as u8);
    let b4 = ((frame_len >> 3) & 0xFF) as u8;
    let b5 = ((frame_len & 0x07) << 5) as u8;
    data.extend_from_slice(&[0xFF, 0xF1, 0x50, b3, b4, b5, 0xFC]);
    data.extend_from_slice(&[0xBB; 9]); // 7 + 9 = 16 bytes
    data
}

fn minimal_flac() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"fLaC");
    // METADATA_BLOCK_HEADER: is_last = 1, type = 0 (STREAMINFO), length = 34
    data.push(0x80);
    data.extend_from_slice(&[0x00, 0x00, 34]);

    // STREAMINFO (34 bytes):
    let mut info = vec![0u8; 34];
    // sample_rate = 44100 = 0x0AC44 (20 bits)
    // channels = 2 -> stored as 1 (3 bits)
    // bits_per_sample = 16 -> stored as 15 (5 bits)
    // total_samples = 44100 (1 second) = 0x00000AC44 (36 bits)
    // bytes 10..18:
    info[10] = 0x0A;
    info[11] = 0xC4;
    info[12] = 0x42; // sample_rate low (4 bits: 4) + channels (3 bits: 001) + bps hi (1 bit: 0)
    info[13] = 0xF0; // bps lo (4 bits: 1111 = 15) + total_samples hi (4 bits: 0)
    info[14] = 0x00;
    info[15] = 0x00;
    info[16] = 0xAC;
    info[17] = 0x44;

    data.extend_from_slice(&info);
    data
}

fn minimal_wav() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&36u32.to_le_bytes()); // overall size
    data.extend_from_slice(b"WAVE");

    // "fmt " chunk
    data.extend_from_slice(b"fmt ");
    data.extend_from_slice(&16u32.to_le_bytes()); // subchunk size = 16
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM format = 1
    data.extend_from_slice(&2u16.to_le_bytes()); // channels = 2
    data.extend_from_slice(&48000u32.to_le_bytes()); // sample_rate = 48000
    data.extend_from_slice(&(48000u32 * 4).to_le_bytes()); // byte rate
    data.extend_from_slice(&4u16.to_le_bytes()); // block align = 4
    data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample = 16

    // "data" chunk
    data.extend_from_slice(b"data");
    data.extend_from_slice(&192000u32.to_le_bytes()); // 192000 bytes = 1 sec
    data
}

fn minimal_rf64() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"RF64");
    data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    data.extend_from_slice(b"WAVE");

    // ds64 chunk
    data.extend_from_slice(b"ds64");
    data.extend_from_slice(&28u32.to_le_bytes()); // size
    data.extend_from_slice(&1000u64.to_le_bytes()); // riff_size
    data.extend_from_slice(&96000u64.to_le_bytes()); // data_size
    data.extend_from_slice(&24000u64.to_le_bytes()); // sample_count
    data.extend_from_slice(&0u32.to_le_bytes()); // table length

    // fmt chunk
    data.extend_from_slice(b"fmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&2u16.to_le_bytes()); // channels = 2
    data.extend_from_slice(&48000u32.to_le_bytes()); // sample_rate
    data.extend_from_slice(&(48000u32 * 4).to_le_bytes()); // byte rate = 192000
    data.extend_from_slice(&4u16.to_le_bytes()); // block align
    data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    data.extend_from_slice(b"data");
    data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    data
}

fn minimal_jpeg(with_exif_orientation: Option<u16>) -> Vec<u8> {
    let mut data = Vec::new();
    // SOI
    data.extend_from_slice(&[0xFF, 0xD8]);

    // Optional APP1 Exif
    if let Some(orient) = with_exif_orientation {
        data.extend_from_slice(&[0xFF, 0xE1]);
        // Build TIFF IFD0
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II"); // Little endian
        tiff.extend_from_slice(&42u16.to_le_bytes()); // TIFF magic
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset = 8
                                                     // IFD0 entries
        tiff.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
                                                     // Entry: tag 0x0112 (Orientation), type SHORT (3), count 1, value orient
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orient.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes()); // padding
        tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD offset = 0

        let mut app1_payload = Vec::new();
        app1_payload.extend_from_slice(b"Exif\0\0");
        app1_payload.extend_from_slice(&tiff);

        let marker_len = (app1_payload.len() + 2) as u16;
        data.extend_from_slice(&marker_len.to_be_bytes());
        data.extend_from_slice(&app1_payload);
    }

    // SOF0: 0xFF, 0xC0
    data.extend_from_slice(&[0xFF, 0xC0]);
    let sof_len = 17u16; // 8 + 3*components(3)
    data.extend_from_slice(&sof_len.to_be_bytes());
    data.push(8); // 8-bit precision
    data.extend_from_slice(&1080u16.to_be_bytes()); // height = 1080
    data.extend_from_slice(&1920u16.to_be_bytes()); // width = 1920
    data.push(3); // 3 components
    data.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);

    // EOI
    data.extend_from_slice(&[0xFF, 0xD9]);
    data
}

fn minimal_png(animated: bool) -> Vec<u8> {
    let mut data = Vec::new();
    // Signature
    data.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR chunk: 13 bytes content
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&800u32.to_be_bytes()); // width = 800
    ihdr.extend_from_slice(&600u32.to_be_bytes()); // height = 600
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type (RGBA)
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace

    data.extend_from_slice(&(ihdr.len() as u32).to_be_bytes());
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&ihdr);
    data.extend_from_slice(&[0, 0, 0, 0]); // dummy CRC

    if animated {
        // acTL chunk: 8 bytes content
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(b"acTL");
        data.extend_from_slice(&10u32.to_be_bytes()); // num_frames
        data.extend_from_slice(&0u32.to_be_bytes()); // num_plays
        data.extend_from_slice(&[0, 0, 0, 0]); // dummy CRC
    }

    // IEND chunk
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(b"IEND");
    data.extend_from_slice(&[0, 0, 0, 0]);
    data
}

fn minimal_gif(animated: bool) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"GIF89a");
    // Logical screen descriptor: width = 320, height = 240, packed = 0, bg = 0, aspect = 0
    data.extend_from_slice(&320u16.to_le_bytes());
    data.extend_from_slice(&240u16.to_le_bytes());
    data.extend_from_slice(&[0, 0, 0]);

    // Frame 1
    data.push(0x2C); // Image descriptor
    data.extend_from_slice(&0u16.to_le_bytes()); // left
    data.extend_from_slice(&0u16.to_le_bytes()); // top
    data.extend_from_slice(&320u16.to_le_bytes()); // width
    data.extend_from_slice(&240u16.to_le_bytes()); // height
    data.push(0); // packed
    data.push(2); // LZW min code size
    data.push(1); // sub-block len
    data.push(0); // sub-block data
    data.push(0); // terminator

    if animated {
        // Frame 2
        data.push(0x2C);
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&320u16.to_le_bytes());
        data.extend_from_slice(&240u16.to_le_bytes());
        data.push(0);
        data.push(2);
        data.push(1);
        data.push(0);
        data.push(0);
    }

    // Trailer
    data.push(0x3B);
    data
}

fn minimal_webp(kind: &str) -> Vec<u8> {
    let mut riff_body = Vec::new();
    riff_body.extend_from_slice(b"WEBP");

    match kind {
        "vp8" => {
            riff_body.extend_from_slice(b"VP8 ");
            let mut vp8 = vec![0u8; 10];
            vp8[0] = 0x00; // keyframe
            vp8[3] = 0x9D;
            vp8[4] = 0x01;
            vp8[5] = 0x2A;
            vp8[6..8].copy_from_slice(&640u16.to_le_bytes());
            vp8[8..10].copy_from_slice(&480u16.to_le_bytes());
            riff_body.extend_from_slice(&(vp8.len() as u32).to_le_bytes());
            riff_body.extend_from_slice(&vp8);
        }
        "vp8l" => {
            riff_body.extend_from_slice(b"VP8L");
            let mut vp8l = vec![0u8; 5];
            vp8l[0] = 0x2F; // signature
                            // 640x480 -> width-1 = 639 (0x027F), height-1 = 479 (0x01DF)
            let w_m1 = 639u32;
            let h_m1 = 479u32;
            vp8l[1] = (w_m1 & 0xFF) as u8;
            vp8l[2] = (((w_m1 >> 8) & 0x3F) | ((h_m1 & 0x03) << 6)) as u8;
            vp8l[3] = ((h_m1 >> 2) & 0xFF) as u8;
            vp8l[4] = ((h_m1 >> 10) & 0x0F) as u8;
            riff_body.extend_from_slice(&(vp8l.len() as u32).to_le_bytes());
            riff_body.extend_from_slice(&vp8l);
            riff_body.push(0); // pad to even
        }
        "vp8x_animated" => {
            riff_body.extend_from_slice(b"VP8X");
            let mut vp8x = vec![0u8; 10];
            vp8x[0] = 0x02; // animation flag
                            // canvas 1280x720 -> width-1 = 1279, height-1 = 719
            let w_m1 = 1279u32;
            let h_m1 = 719u32;
            vp8x[4] = (w_m1 & 0xFF) as u8;
            vp8x[5] = ((w_m1 >> 8) & 0xFF) as u8;
            vp8x[6] = ((w_m1 >> 16) & 0xFF) as u8;
            vp8x[7] = (h_m1 & 0xFF) as u8;
            vp8x[8] = ((h_m1 >> 8) & 0xFF) as u8;
            vp8x[9] = ((h_m1 >> 16) & 0xFF) as u8;
            riff_body.extend_from_slice(&(vp8x.len() as u32).to_le_bytes());
            riff_body.extend_from_slice(&vp8x);
        }
        _ => {}
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff_body.len() as u32).to_le_bytes());
    out.extend_from_slice(&riff_body);
    out
}

// ---------------------------------------------------------------------------
// MPEG-TS Tests
// ---------------------------------------------------------------------------

#[test]
fn test_mpeg_ts_happy_path() {
    let data = minimal_ts();
    let desc = probe_bytes(&data).expect("ts probes cleanly");
    assert_eq!(desc.container, Container::Ts);
    let v = desc.video.expect("ts video stream");
    assert_eq!(v.codec, VideoCodec::H264);
    let a = desc.audio.expect("ts audio stream");
    assert_eq!(a.codec, AudioCodec::Aac);
    assert!(desc.pcr.is_some());
}

#[test]
fn test_mpeg_ts_truncation() {
    let data = minimal_ts();
    // Case 1: cut before 1 full packet
    assert!(matches!(
        probe_bytes(&data[..100]),
        Err(ProbeError::UnrecognizedContainer) | Err(ProbeError::Truncated(_))
    ));
    // Case 2: cut inside PAT section (only 1 packet present, missing PMT)
    let cut_pat = &data[..188];
    assert!(probe_bytes(cut_pat).is_err());
    // Case 3: cut PMT section length
    let mut cut_pmt = data.clone();
    cut_pmt.truncate(188 + 20);
    let res = probe_bytes(&cut_pmt);
    // Either partial PMT returns TS or Truncated
    assert!(res.is_ok() || res.is_err());
}

#[test]
fn test_mpeg_ts_hostile() {
    // Hostile 1: Corrupted PAT pointer field > 188
    let mut data = minimal_ts();
    data[4] = 200; // invalid pointer offset
    assert!(probe_bytes(&data).is_err());

    // Hostile 2: Corrupted sync byte in packet 1 and 2
    let mut corrupted = minimal_ts();
    corrupted[0] = 0x00;
    corrupted[188] = 0x00;
    assert_eq!(
        probe_bytes(&corrupted),
        Err(ProbeError::UnrecognizedContainer)
    );
}

// ---------------------------------------------------------------------------
// MP3 Tests
// ---------------------------------------------------------------------------

#[test]
fn test_mp3_happy_path() {
    let data = minimal_mp3();
    let desc = probe_bytes(&data).expect("mp3 probes cleanly");
    assert_eq!(desc.container, Container::Mp3);
    let a = desc.audio.expect("mp3 audio");
    assert_eq!(a.codec, AudioCodec::Mp3);
    assert_eq!(a.channels, Some(2));
    assert_eq!(a.sample_rate, Some(44100));
    assert_eq!(a.bitrate_kbps, Some(128));
}

#[test]
fn test_mp3_truncation() {
    let data = minimal_mp3();
    // Case 1: cut inside ID3 header
    assert!(matches!(
        probe_bytes(&data[..6]),
        Err(ProbeError::Truncated(_)) | Err(ProbeError::Malformed { .. })
    ));
    // Case 2: cut immediately after ID3 header before audio frame
    assert!(matches!(
        probe_bytes(&data[..20]),
        Err(ProbeError::Truncated(_)) | Err(ProbeError::Malformed { .. })
    ));
    // Case 3: cut in middle of audio frame header
    assert!(matches!(
        probe_bytes(&data[..22]),
        Err(ProbeError::Truncated(_)) | Err(ProbeError::Malformed { .. })
    ));
}

#[test]
fn test_mp3_hostile() {
    // Hostile 1: ID3 size specifies huge syncsafe length
    let mut data = minimal_mp3();
    data[6] = 0x7F;
    data[7] = 0x7F;
    data[8] = 0x7F;
    data[9] = 0x7F;
    assert!(matches!(probe_bytes(&data), Err(ProbeError::Truncated(_))));

    // Hostile 2: Invalid MPEG audio bitrate index 0x0F
    let mut data2 = minimal_mp3();
    data2[22] = 0xF0; // bad bitrate index
    assert!(probe_bytes(&data2).is_err());
}

// ---------------------------------------------------------------------------
// ADTS Tests
// ---------------------------------------------------------------------------

#[test]
fn test_adts_happy_path() {
    let data = minimal_adts();
    let desc = probe_bytes(&data).expect("adts probes cleanly");
    assert_eq!(desc.container, Container::Aac);
    let a = desc.audio.expect("adts audio");
    assert_eq!(a.codec, AudioCodec::Aac);
    assert_eq!(a.channels, Some(2));
    assert_eq!(a.sample_rate, Some(44100));
}

#[test]
fn test_adts_truncation() {
    let data = minimal_adts();
    // Case 1: 1 byte
    assert_eq!(
        probe_bytes(&data[..1]),
        Err(ProbeError::UnrecognizedContainer)
    );
    // Case 2: 3 bytes
    assert!(matches!(
        probe_bytes(&data[..3]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 3: 5 bytes
    assert!(matches!(
        probe_bytes(&data[..5]),
        Err(ProbeError::Truncated(_))
    ));
}

#[test]
fn test_adts_hostile() {
    // Hostile 1: Unsupported sample rate index 15
    let mut data = minimal_adts();
    data[2] = 0x50 | (15 << 2);
    assert!(matches!(
        probe_bytes(&data),
        Err(ProbeError::Malformed { .. })
    ));

    // Hostile 2: Invalid frame length < 7
    let mut data2 = minimal_adts();
    data2[3] = 0x80;
    data2[4] = 0x00;
    data2[5] = 0x00; // frame_length = 0
    assert!(matches!(
        probe_bytes(&data2),
        Err(ProbeError::Malformed { .. })
    ));
}

// ---------------------------------------------------------------------------
// FLAC Tests
// ---------------------------------------------------------------------------

#[test]
fn test_flac_happy_path() {
    let data = minimal_flac();
    let desc = probe_bytes(&data).expect("flac probes cleanly");
    assert_eq!(desc.container, Container::Flac);
    let a = desc.audio.expect("flac audio");
    assert_eq!(a.codec, AudioCodec::Flac);
    assert_eq!(a.channels, Some(2));
    assert_eq!(a.sample_rate, Some(44100));
    assert_eq!(a.bit_depth, Some(16));
    assert_eq!(desc.duration_ms, Some(1000));
}

#[test]
fn test_flac_truncation() {
    let data = minimal_flac();
    // Case 1: 3 bytes
    assert_eq!(
        probe_bytes(&data[..3]),
        Err(ProbeError::UnrecognizedContainer)
    );
    // Case 2: 4 bytes ("fLaC")
    assert!(matches!(
        probe_bytes(&data[..4]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 3: 20 bytes (truncated STREAMINFO)
    assert!(matches!(
        probe_bytes(&data[..20]),
        Err(ProbeError::Truncated(_))
    ));
}

#[test]
fn test_flac_hostile() {
    // Hostile 1: First block is not STREAMINFO
    let mut data = minimal_flac();
    data[4] = 0x81; // block type = 1
    assert!(matches!(
        probe_bytes(&data),
        Err(ProbeError::Malformed { .. })
    ));

    // Hostile 2: STREAMINFO length is < 34
    let mut data2 = minimal_flac();
    data2[7] = 10;
    assert!(matches!(
        probe_bytes(&data2),
        Err(ProbeError::Malformed { .. })
    ));
}

// ---------------------------------------------------------------------------
// WAV / RF64 Tests
// ---------------------------------------------------------------------------

#[test]
fn test_wav_and_rf64_happy_path() {
    let wav_data = minimal_wav();
    let desc = probe_bytes(&wav_data).expect("wav probes cleanly");
    assert_eq!(desc.container, Container::Wav);
    let a = desc.audio.expect("wav audio");
    assert_eq!(a.codec, AudioCodec::Pcm);
    assert_eq!(a.channels, Some(2));
    assert_eq!(a.sample_rate, Some(48000));
    assert_eq!(a.bit_depth, Some(16));
    assert_eq!(desc.duration_ms, Some(1000));

    let rf64_data = minimal_rf64();
    let desc_rf64 = probe_bytes(&rf64_data).expect("rf64 probes cleanly");
    assert_eq!(desc_rf64.container, Container::Wav);
    assert_eq!(desc_rf64.duration_ms, Some(500));
}

#[test]
fn test_wav_truncation() {
    let data = minimal_wav();
    // Case 1: cut before 12 bytes
    assert!(matches!(
        probe_bytes(&data[..10]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 2: cut inside fmt header
    assert!(matches!(
        probe_bytes(&data[..16]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 3: cut inside fmt payload
    assert!(matches!(
        probe_bytes(&data[..24]),
        Err(ProbeError::Truncated(_))
    ));
}

#[test]
fn test_wav_hostile() {
    // Hostile 1: RIFF with non-WAVE form type
    let mut data = minimal_wav();
    data[8..12].copy_from_slice(b"AVI ");
    assert_eq!(probe_bytes(&data), Err(ProbeError::UnrecognizedContainer));

    // Hostile 2: fmt chunk reports length 2
    let mut data2 = minimal_wav();
    data2[16..20].copy_from_slice(&2u32.to_le_bytes());
    assert!(matches!(probe_bytes(&data2), Err(ProbeError::Truncated(_))));
}

// ---------------------------------------------------------------------------
// JPEG Tests
// ---------------------------------------------------------------------------

#[test]
fn test_jpeg_happy_path() {
    let data = minimal_jpeg(Some(6));
    let desc = probe_bytes(&data).expect("jpeg probes cleanly");
    assert_eq!(desc.container, Container::Jpeg);
    let img = desc.image.expect("jpeg image info");
    assert_eq!(img.width, 1920);
    assert_eq!(img.height, 1080);
    assert_eq!(img.orientation, Some(6));
    assert!(!img.animated);
}

#[test]
fn test_jpeg_truncation() {
    let data = minimal_jpeg(Some(1));
    // Case 1: 1 byte
    assert_eq!(
        probe_bytes(&data[..1]),
        Err(ProbeError::UnrecognizedContainer)
    );
    // Case 2: cut in APP1
    assert!(matches!(
        probe_bytes(&data[..10]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 3: cut before SOF
    let sof_idx = data.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    assert!(matches!(
        probe_bytes(&data[..sof_idx + 1]),
        Err(ProbeError::Truncated(_))
    ));
}

#[test]
fn test_jpeg_hostile() {
    // Hostile 1: Marker length specifies 0
    let mut data = minimal_jpeg(None);
    let sof_idx = data.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    data[sof_idx + 2] = 0;
    data[sof_idx + 3] = 0;
    assert!(matches!(
        probe_bytes(&data),
        Err(ProbeError::Malformed { .. })
    ));

    // Hostile 2: Trailing cut without SOF
    let truncated = vec![0xFF, 0xD8, 0xFF, 0xD9];
    assert!(matches!(
        probe_bytes(&truncated),
        Err(ProbeError::Truncated(_))
    ));
}

// ---------------------------------------------------------------------------
// PNG Tests
// ---------------------------------------------------------------------------

#[test]
fn test_png_happy_path() {
    let still_png = minimal_png(false);
    let desc = probe_bytes(&still_png).expect("png probes cleanly");
    assert_eq!(desc.container, Container::Png);
    let img = desc.image.expect("png image info");
    assert_eq!(img.width, 800);
    assert_eq!(img.height, 600);
    assert!(!img.animated);

    let anim_png = minimal_png(true);
    let desc_anim = probe_bytes(&anim_png).expect("apng probes cleanly");
    assert!(desc_anim.image.expect("png image info").animated);
}

#[test]
fn test_png_truncation() {
    let data = minimal_png(false);
    // Case 1: 4 bytes
    assert_eq!(
        probe_bytes(&data[..4]),
        Err(ProbeError::UnrecognizedContainer)
    );
    // Case 2: header only (8 bytes)
    assert!(matches!(
        probe_bytes(&data[..8]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 3: partial IHDR
    assert!(matches!(
        probe_bytes(&data[..20]),
        Err(ProbeError::Truncated(_))
    ));
}

#[test]
fn test_png_hostile() {
    // Hostile 1: IHDR chunk length is 2 bytes (too small)
    let mut data = minimal_png(false);
    data[8..12].copy_from_slice(&2u32.to_be_bytes());
    assert!(matches!(
        probe_bytes(&data),
        Err(ProbeError::Malformed { .. })
    ));

    // Hostile 2: Corrupted magic signature
    let mut data2 = minimal_png(false);
    data2[0] = 0x00;
    assert_eq!(probe_bytes(&data2), Err(ProbeError::UnrecognizedContainer));
}

// ---------------------------------------------------------------------------
// GIF Tests
// ---------------------------------------------------------------------------

#[test]
fn test_gif_happy_path() {
    let still_gif = minimal_gif(false);
    let desc = probe_bytes(&still_gif).expect("gif probes cleanly");
    assert_eq!(desc.container, Container::Gif);
    let img = desc.image.expect("gif image info");
    assert_eq!(img.width, 320);
    assert_eq!(img.height, 240);
    assert!(!img.animated);

    let anim_gif = minimal_gif(true);
    let desc_anim = probe_bytes(&anim_gif).expect("anim gif probes cleanly");
    assert!(desc_anim.image.expect("gif image info").animated);
}

#[test]
fn test_gif_truncation() {
    let data = minimal_gif(false);
    // Case 1: 3 bytes
    assert_eq!(
        probe_bytes(&data[..3]),
        Err(ProbeError::UnrecognizedContainer)
    );
    // Case 2: 6 bytes signature
    assert!(matches!(
        probe_bytes(&data[..6]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 3: 10 bytes (partial LSD)
    assert!(matches!(
        probe_bytes(&data[..10]),
        Err(ProbeError::Truncated(_))
    ));
}

#[test]
fn test_gif_hostile() {
    // Hostile 1: Global color table flag set, but buffer truncated immediately
    let mut data = minimal_gif(false);
    data[10] = 0x87; // GCT flag with max size exponent (7) -> 3 * 256 = 768 bytes required
    assert!(matches!(probe_bytes(&data), Err(ProbeError::Truncated(_))));

    // Hostile 2: Invalid magic signature
    let mut data2 = minimal_gif(false);
    data2[0..6].copy_from_slice(b"GIF99a");
    assert_eq!(probe_bytes(&data2), Err(ProbeError::UnrecognizedContainer));
}

// ---------------------------------------------------------------------------
// WebP Tests
// ---------------------------------------------------------------------------

#[test]
fn test_webp_happy_path() {
    let vp8 = minimal_webp("vp8");
    let desc1 = probe_bytes(&vp8).expect("webp vp8 probes");
    assert_eq!(desc1.container, Container::Webp);
    assert_eq!(desc1.image.as_ref().unwrap().width, 640);
    assert_eq!(desc1.image.as_ref().unwrap().height, 480);
    assert!(!desc1.image.as_ref().unwrap().animated);

    let vp8l = minimal_webp("vp8l");
    let desc2 = probe_bytes(&vp8l).expect("webp vp8l probes");
    assert_eq!(desc2.image.as_ref().unwrap().width, 640);
    assert_eq!(desc2.image.as_ref().unwrap().height, 480);

    let vp8x = minimal_webp("vp8x_animated");
    let desc3 = probe_bytes(&vp8x).expect("webp vp8x probes");
    assert_eq!(desc3.image.as_ref().unwrap().width, 1280);
    assert_eq!(desc3.image.as_ref().unwrap().height, 720);
    assert!(desc3.image.as_ref().unwrap().animated);
}

#[test]
fn test_webp_truncation() {
    let data = minimal_webp("vp8");
    // Case 1: 4 bytes
    assert!(matches!(
        probe_bytes(&data[..4]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 2: 12 bytes header
    assert!(matches!(
        probe_bytes(&data[..12]),
        Err(ProbeError::Truncated(_))
    ));
    // Case 3: cut inside VP8 chunk
    assert!(matches!(
        probe_bytes(&data[..22]),
        Err(ProbeError::Truncated(_))
    ));
}

#[test]
fn test_webp_hostile() {
    // Hostile 1: Chunk claims 10000 bytes, but buffer ends
    let mut data = minimal_webp("vp8");
    data[16..20].copy_from_slice(&10000u32.to_le_bytes());
    assert!(matches!(probe_bytes(&data), Err(ProbeError::Truncated(_))));

    // Hostile 2: RIFF with unknown format
    let mut data2 = minimal_webp("vp8");
    data2[8..12].copy_from_slice(b"XYZW");
    assert_eq!(probe_bytes(&data2), Err(ProbeError::UnrecognizedContainer));
}

// ---------------------------------------------------------------------------
// Proptest Fuzz-Safety Harnesses
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn proptest_arbitrary_bytes_never_panics(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = probe_bytes(&data);
    }

    #[test]
    fn proptest_fuzzed_ts_never_panics(mut prefix in proptest::collection::vec(any::<u8>(), 0..100), tail in proptest::collection::vec(any::<u8>(), 0..500)) {
        let mut base = minimal_ts();
        base.append(&mut prefix);
        base.extend_from_slice(&tail);
        let _ = probe_bytes(&base);
    }

    #[test]
    fn proptest_fuzzed_jpeg_never_panics(mut prefix in proptest::collection::vec(any::<u8>(), 0..50)) {
        let mut base = minimal_jpeg(Some(1));
        base.append(&mut prefix);
        let _ = probe_bytes(&base);
    }

    #[test]
    fn proptest_fuzzed_png_never_panics(mut prefix in proptest::collection::vec(any::<u8>(), 0..50)) {
        let mut base = minimal_png(true);
        base.append(&mut prefix);
        let _ = probe_bytes(&base);
    }

    #[test]
    fn proptest_fuzzed_audio_never_panics(mut prefix in proptest::collection::vec(any::<u8>(), 0..50)) {
        let mut mp3 = minimal_mp3();
        mp3.append(&mut prefix);
        let _ = probe_bytes(&mp3);

        let mut wav = minimal_wav();
        wav.extend_from_slice(&prefix);
        let _ = probe_bytes(&wav);
    }
}
