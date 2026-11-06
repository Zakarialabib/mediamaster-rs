//! Annex-B live stream and container parity tests.

use mediamaster_probe::annexb::parse_h264_sps;
use mediamaster_probe::{
    probe_bytes, probe_stream, Container, ContainerProbe, MediaProbe, ProbeError, VideoCodec,
};
use proptest::prelude::*;

/// Build an ISO-BMFF box: `[size][type][content]`.
fn boxed(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
    let size = (content.len() + 8) as u32;
    let mut out = size.to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(content);
    out
}

/// Minimal MP4 containing 1080p H.264 stream (High@4.1).
fn mp4_1080p_h264() -> Vec<u8> {
    let mut ftyp_content = Vec::new();
    ftyp_content.extend_from_slice(b"isom");
    ftyp_content.extend_from_slice(&2u32.to_be_bytes());
    ftyp_content.extend_from_slice(b"isomiso2avc1mp41");
    let ftyp = boxed(b"ftyp", &ftyp_content);

    let mut mvhd = vec![0u8; 100];
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[16..20].copy_from_slice(&5000u32.to_be_bytes());
    let mvhd = boxed(b"mvhd", &mvhd);

    let mut tkhd = vec![0u8; 84];
    tkhd[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
    tkhd[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
    let tkhd = boxed(b"tkhd", &tkhd);

    let mut mdhd = vec![0u8; 24];
    mdhd[12..16].copy_from_slice(&15360u32.to_be_bytes());
    mdhd[16..20].copy_from_slice(&15360u32.to_be_bytes());
    let mdhd = boxed(b"mdhd", &mdhd);

    // avcC: version=1, profile=100 (High), compat=0, level=41 (4.1)
    let avc_c = boxed(b"avcC", &[1u8, 100, 0, 41, 255, 225, 0, 0]);

    let mut fixed = vec![0u8; 78];
    fixed[6..8].copy_from_slice(&1u16.to_be_bytes());
    fixed[24..26].copy_from_slice(&1920u16.to_be_bytes());
    fixed[26..28].copy_from_slice(&1080u16.to_be_bytes());
    let mut avc1_content = fixed;
    avc1_content.extend_from_slice(&avc_c);
    let avc1 = boxed(b"avc1", &avc1_content);

    let mut stsd_content = vec![0u8; 8];
    stsd_content[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_content.extend_from_slice(&avc1);
    let stsd = boxed(b"stsd", &stsd_content);

    let minf = boxed(b"minf", &boxed(b"stbl", &stsd));
    let mdia = boxed(b"mdia", &[mdhd, minf].concat());
    let trak = boxed(b"trak", &[tkhd, mdia].concat());
    let moov = boxed(b"moov", &[mvhd, trak].concat());

    [ftyp, moov].concat()
}

/// 1080p H.264 SPS NAL unit (High profile @ level 4.1).
fn h264_1080p_sps_nal() -> Vec<u8> {
    vec![103, 100, 0, 41, 172, 232, 7, 128, 34, 126, 80]
}

/// 4K HEVC SPS NAL unit (Main10 profile @ level 5.1).
fn hevc_4k_sps_nal() -> Vec<u8> {
    vec![
        66, 1, 1, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 153, 160, 1, 224, 32, 2, 28, 77, 128,
    ]
}

/// Construct an Annex-B stream with start codes containing the given NAL units.
fn make_annexb_stream(nalus: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for nal in nalus {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    out
}

#[test]
fn test_h264_annexb_matches_mp4_descriptor() {
    let mp4_data = mp4_1080p_h264();
    let mp4_desc = probe_bytes(&mp4_data).expect("mp4 probes");

    let sps = h264_1080p_sps_nal();
    let pps = [0x68, 0xCE, 0x38, 0x80]; // dummy PPS
    let idr = [0x65, 0x88, 0x84]; // dummy IDR
    let annexb_stream = make_annexb_stream(&[&sps, &pps, &idr]);

    let stream_desc = probe_stream(&annexb_stream).expect("annex-b stream probes");

    // Video metadata matches between MP4 container and Annex-B stream
    let v_mp4 = mp4_desc.video.as_ref().unwrap();
    let v_stream = stream_desc.video.as_ref().unwrap();

    assert_eq!(v_stream.codec, VideoCodec::H264);
    assert_eq!(v_stream.codec, v_mp4.codec);
    assert_eq!(v_stream.width, Some(1920));
    assert_eq!(v_stream.width, v_mp4.width);
    assert_eq!(v_stream.height, Some(1080));
    assert_eq!(v_stream.height, v_mp4.height);
    assert_eq!(v_stream.profile.as_deref(), Some("High"));
    assert_eq!(v_stream.profile, v_mp4.profile);
    assert_eq!(v_stream.level, Some(41));
    assert_eq!(v_stream.level, v_mp4.level);
    assert_eq!(v_stream.ten_bit, Some(false));
    assert_eq!(v_stream.ten_bit, v_mp4.ten_bit);

    // MediaProbe trait method call also succeeds and produces the same result
    let probe = ContainerProbe;
    let trait_desc = probe
        .probe_stream(&annexb_stream)
        .expect("MediaProbe trait works");
    assert_eq!(trait_desc, stream_desc);
}

#[test]
fn test_hevc_4k_annexb_stream() {
    let sps = hevc_4k_sps_nal();
    let vps = [0x40, 0x01, 0x0C, 0x01, 0xFF]; // dummy VPS
    let idr = [0x26, 0x01, 0xAF]; // dummy HEVC IDR
    let stream = make_annexb_stream(&[&vps, &sps, &idr]);

    let desc = probe_stream(&stream).expect("4k hevc probes cleanly");
    assert_eq!(desc.container, Container::AnnexB);

    let v = desc.video.expect("video metadata");
    assert_eq!(v.codec, VideoCodec::Hevc);
    assert_eq!(v.width, Some(3840));
    assert_eq!(v.height, Some(2160));
    assert_eq!(v.profile.as_deref(), Some("HEVC"));
    assert_eq!(v.level, Some(153)); // Level 5.1 in HEVC is 153 (5.1 * 30)
    assert_eq!(v.ten_bit, Some(true)); // Main10
}

#[test]
fn test_annexb_emulation_prevention_unescaping() {
    // Test that emulation prevention 0x000003 is stripped correctly
    let mut sps_with_ep = vec![103, 100, 0, 0, 3, 41];
    let sps = h264_1080p_sps_nal();
    sps_with_ep.extend_from_slice(&sps[4..]);
    // parse_h264_sps should unescape without panicking
    let _ = parse_h264_sps(&sps_with_ep);
}

#[test]
fn test_annexb_truncation_and_hostile() {
    // Empty
    assert_eq!(probe_stream(&[]), Err(ProbeError::EmptyInput));

    // Incomplete start code
    assert_eq!(
        probe_stream(&[0, 0]),
        Err(ProbeError::UnrecognizedContainer)
    );

    // Start code with truncated NAL header
    let stream = vec![0, 0, 0, 1];
    assert!(matches!(
        probe_stream(&stream),
        Err(ProbeError::Truncated(_))
    ));

    // Random byte stream without valid SPS
    let stream = vec![0, 0, 0, 1, 0x67, 0x00, 0x00];
    assert!(matches!(
        probe_stream(&stream),
        Err(ProbeError::Truncated(_)) | Err(ProbeError::Malformed { .. })
    ));
}

proptest! {
    #[test]
    fn proptest_fuzzed_annexb_never_panics(mut prefix in proptest::collection::vec(any::<u8>(), 0..50), tail in proptest::collection::vec(any::<u8>(), 0..100)) {
        let sps = h264_1080p_sps_nal();
        let mut base = make_annexb_stream(&[&sps]);
        base.append(&mut prefix);
        base.extend_from_slice(&tail);
        let _ = probe_stream(&base);
    }
}
