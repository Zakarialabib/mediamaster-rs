//! End-to-end probe tests against hand-crafted minimal MP4 and WebM fixtures.

use mediamaster_probe::{probe_bytes, AudioCodec, Container, ProbeError, VideoCodec};

/// Build an ISO-BMFF box: `[size][type][content]`.
fn boxed(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
    let size = (content.len() + 8) as u32;
    let mut out = size.to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(content);
    out
}

/// Build an EBML element: `[id bytes][1-byte size VINT][content]`.
/// Only supports contents shorter than 127 bytes (enough for fixtures).
fn ebml(id: &[u8], content: &[u8]) -> Vec<u8> {
    assert!(
        content.len() < 127,
        "fixture content must fit a 1-byte VINT"
    );
    let mut out = id.to_vec();
    out.push(0x80 | content.len() as u8);
    out.extend_from_slice(content);
    out
}

/// A minimal MP4: `ftyp` + `moov` with one H.264 video track.
///
/// Expected: 1920x1080, High@4.1, 30 fps, 5000 ms, brand `isom`.
fn minimal_mp4() -> Vec<u8> {
    // ftyp: major=isom, minor=2, compatible=isom/iso2/avc1/mp41
    let mut ftyp_content = Vec::new();
    ftyp_content.extend_from_slice(b"isom");
    ftyp_content.extend_from_slice(&2u32.to_be_bytes());
    ftyp_content.extend_from_slice(b"isomiso2avc1mp41");
    let ftyp = boxed(b"ftyp", &ftyp_content);

    // mvhd (v0): timescale=1000, duration=5000 => 5000 ms
    let mut mvhd = vec![0u8; 100];
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[16..20].copy_from_slice(&5000u32.to_be_bytes());
    let mvhd = boxed(b"mvhd", &mvhd);

    // tkhd (v0): width/height are 16.16 fixed at offsets 76/80.
    let mut tkhd = vec![0u8; 84];
    tkhd[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
    tkhd[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
    let tkhd = boxed(b"tkhd", &tkhd);

    // mdhd (v0): timescale=15360 (fps base)
    let mut mdhd = vec![0u8; 24];
    mdhd[12..16].copy_from_slice(&15360u32.to_be_bytes());
    mdhd[16..20].copy_from_slice(&15360u32.to_be_bytes());
    let mdhd = boxed(b"mdhd", &mdhd);

    // avcC: version=1, profile=100 (High), compat=0, level=41 (4.1)
    let avc_c = boxed(b"avcC", &[1u8, 100, 0, 41, 255, 225, 0, 0]);

    // avc1 visual sample entry: 78-byte fixed header (width@24, height@26)
    // then child boxes (avcC) starting at entry offset 86.
    let mut fixed = vec![0u8; 78];
    fixed[6..8].copy_from_slice(&1u16.to_be_bytes()); // data_reference_index
    fixed[24..26].copy_from_slice(&1920u16.to_be_bytes());
    fixed[26..28].copy_from_slice(&1080u16.to_be_bytes());
    let mut avc1_content = fixed;
    avc1_content.extend_from_slice(&avc_c);
    let avc1 = boxed(b"avc1", &avc1_content);

    // stsd: version+flags(4) + entry_count(4) + entries
    let mut stsd_content = vec![0u8; 8];
    stsd_content[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_content.extend_from_slice(&avc1);
    let stsd = boxed(b"stsd", &stsd_content);

    // stts: count=1, sample_count=30, delta=512 => 15360/512 = 30 fps
    let mut stts_content = Vec::new();
    stts_content.extend_from_slice(&[0u8; 4]);
    stts_content.extend_from_slice(&1u32.to_be_bytes());
    stts_content.extend_from_slice(&30u32.to_be_bytes());
    stts_content.extend_from_slice(&512u32.to_be_bytes());
    let stts = boxed(b"stts", &stts_content);

    let stbl = boxed(b"stbl", &[stsd, stts].concat());
    let minf = boxed(b"minf", &stbl);
    let mdia = boxed(b"mdia", &[mdhd, minf].concat());
    let trak = boxed(b"trak", &[tkhd, mdia].concat());
    let moov = boxed(b"moov", &[mvhd, trak].concat());

    [ftyp, moov].concat()
}

/// A minimal WebM (EBML): DocType `webm`, Duration 5000 ticks, one H.264
/// video track 1920x1080.
fn minimal_webm() -> Vec<u8> {
    let doctype = ebml(&[0x42, 0x82], b"webm");
    let mut ebml_header = vec![0x1A, 0x45, 0xDF, 0xA3];
    ebml_header.push(0x80 | doctype.len() as u8);
    ebml_header.extend_from_slice(&doctype);

    // Duration (float, 8 bytes) = 5000 ticks; default timecode_scale = 1ms.
    let duration = ebml(&[0x44, 0x89], &5000.0f64.to_be_bytes());
    let info = ebml(&[0x15, 0x49, 0xA9, 0x66], &duration);

    let pix_w = ebml(&[0xB0], &[0x07, 0x80]); // 1920
    let pix_h = ebml(&[0xBA], &[0x04, 0x38]); // 1080
    let video = ebml(&[0xE0], &[pix_w, pix_h].concat());

    let track_type = ebml(&[0x83], &[1]); // video
    let codec_id = ebml(&[0x86], b"V_MPEG4/ISO/AVC");
    let track_entry = ebml(&[0xAE], &[track_type, codec_id, video].concat());
    let tracks = ebml(&[0x16, 0x54, 0xAE, 0x6B], &track_entry);

    // Segment with unknown-size VINT (0xFF) => extends to end of buffer.
    let mut segment = vec![0x18, 0x53, 0x80, 0x67, 0xFF];
    segment.extend_from_slice(&info);
    segment.extend_from_slice(&tracks);

    [ebml_header, segment].concat()
}

#[test]
fn probe_mp4_container_and_brand() {
    let desc = probe_bytes(&minimal_mp4()).expect("mp4 probes");
    assert_eq!(desc.container, Container::Mp4);
    assert_eq!(desc.brand.as_deref(), Some("isom"));
    assert_eq!(desc.duration_ms, Some(5000));
}

#[test]
fn probe_mp4_video_codec_dimensions_and_fps() {
    let desc = probe_bytes(&minimal_mp4()).expect("mp4 probes");
    let v = desc.video.expect("video track");
    assert_eq!(v.codec, VideoCodec::H264);
    assert_eq!(v.width, Some(1920));
    assert_eq!(v.height, Some(1080));
    assert_eq!(v.fps, Some(30.0));
    assert_eq!(v.profile.as_deref(), Some("High"));
    assert_eq!(v.level, Some(41));
    assert_eq!(v.ten_bit, Some(false));
    assert!(desc.audio.is_none());
}

#[test]
fn probe_webm_container_codec_and_duration() {
    let desc = probe_bytes(&minimal_webm()).expect("webm probes");
    assert_eq!(desc.container, Container::Webm);
    assert_eq!(desc.brand.as_deref(), Some("webm"));
    assert_eq!(desc.duration_ms, Some(5000));
    let v = desc.video.expect("video track");
    assert_eq!(v.codec, VideoCodec::H264);
    assert_eq!(v.width, Some(1920));
    assert_eq!(v.height, Some(1080));
}

#[test]
fn probe_dispatches_by_magic() {
    assert_eq!(
        probe_bytes(&minimal_mp4()).unwrap().container,
        Container::Mp4
    );
    assert_eq!(
        probe_bytes(&minimal_webm()).unwrap().container,
        Container::Webm
    );
}

#[test]
fn probe_errors_on_empty_and_unknown() {
    assert_eq!(probe_bytes(&[]), Err(ProbeError::EmptyInput));
    assert_eq!(
        probe_bytes(&[0u8; 16]),
        Err(ProbeError::UnrecognizedContainer)
    );
}

// Keep the audio codec type exercised so a future audio-fixture test can
// compile against it without dead-code warnings.
#[allow(dead_code)]
fn _audio_codec_smoke() -> AudioCodec {
    AudioCodec::Aac
}
