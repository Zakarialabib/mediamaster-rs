//! H.264 (AVC) and H.265 (HEVC) Annex-B elementary-stream parser.
//!
//! Parses NAL units delimited by 3-byte (`0x000001`) or 4-byte (`0x00000001`)
//! start codes into SPS/PPS/VPS parameters, producing codec profile, level,
//! bit depth (10-bit), and dimensions without container overhead.

use crate::error::ProbeError;
use crate::types::{Container, MediaDescriptor, VideoCodec, VideoInfo};

/// Parse an Annex-B elementary stream (H.264 or H.265) into a `MediaDescriptor`.
pub fn probe_annexb(data: &[u8]) -> Result<MediaDescriptor, ProbeError> {
    if data.is_empty() {
        return Err(ProbeError::EmptyInput);
    }

    let nalus = extract_nal_units(data);
    if nalus.is_empty() {
        return Err(ProbeError::Truncated("annexb"));
    }

    // Scan for HEVC SPS (type 33) or VPS (type 32)
    for nalu in &nalus {
        if nalu.len() >= 2 {
            let hevc_type = (nalu[0] >> 1) & 0x3F;
            if hevc_type == 33 {
                // Potential HEVC SPS
                if let Ok(info) = parse_hevc_sps(nalu) {
                    let mut desc = MediaDescriptor::new(Container::AnnexB);
                    desc.video = Some(info);
                    return Ok(desc);
                }
            }
        }
    }

    // Then scan for H.264 SPS (type 7)
    for nalu in &nalus {
        if !nalu.is_empty() {
            let h264_type = nalu[0] & 0x1F;
            if h264_type == 7 {
                // Potential H.264 SPS
                if let Ok(info) = parse_h264_sps(nalu) {
                    let mut desc = MediaDescriptor::new(Container::AnnexB);
                    desc.video = Some(info);
                    return Ok(desc);
                }
            }
        }
    }

    // If start codes existed but no valid SPS could be parsed
    Err(ProbeError::Truncated("annexb"))
}

/// Quick check if the stream contains Annex-B start codes.
pub fn is_annexb(data: &[u8]) -> bool {
    find_start_code(data, 0).is_some()
}

/// Find the next Annex-B start code `0x000001` or `0x00000001`.
/// Returns `(start_index_of_nalu, start_code_length)`.
fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    if from + 3 > data.len() {
        return None;
    }
    let mut i = from;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                return Some((i + 3, 3));
            } else if data[i + 2] == 0 && i + 4 <= data.len() && data[i + 3] == 1 {
                return Some((i + 4, 4));
            }
        }
        i += 1;
    }
    None
}

/// Extract all NAL units (without start codes) from `data`.
fn extract_nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut nalus = Vec::new();
    let mut offset = 0;

    let mut current_start = None;
    while offset < data.len() {
        if let Some((nal_start, _code_len)) = find_start_code(data, offset) {
            if let Some(prev_start) = current_start {
                let end = nal_start - _code_len;
                // Trim trailing zeros before start code if any
                let mut nal_end = end;
                while nal_end > prev_start && data[nal_end - 1] == 0 {
                    nal_end -= 1;
                }
                if nal_end > prev_start {
                    nalus.push(&data[prev_start..nal_end]);
                }
            }
            current_start = Some(nal_start);
            offset = nal_start;
        } else {
            break;
        }
    }

    if let Some(prev_start) = current_start {
        let mut nal_end = data.len();
        while nal_end > prev_start && data[nal_end - 1] == 0 {
            nal_end -= 1;
        }
        if nal_end > prev_start {
            nalus.push(&data[prev_start..nal_end]);
        }
    }

    nalus
}

/// Strip emulation prevention bytes (`0x000003` -> `0x0000`).
fn unescape_rbsp(nalu: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(nalu.len());
    let mut i = 0;
    while i < nalu.len() {
        if i + 2 < nalu.len() && nalu[i] == 0 && nalu[i + 1] == 0 && nalu[i + 2] == 3 {
            rbsp.push(0);
            rbsp.push(0);
            i += 3; // skip emulation prevention byte (0x03)
        } else {
            rbsp.push(nalu[i]);
            i += 1;
        }
    }
    rbsp
}

/// Bit reader for parsing Exp-Golomb and bit-level fields.
struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    fn read_bit(&mut self) -> Result<u32, ProbeError> {
        let byte_pos = self.bit_pos / 8;
        if byte_pos >= self.data.len() {
            return Err(ProbeError::Truncated("bitstream"));
        }
        let bit_in_byte = 7 - (self.bit_pos % 8);
        self.bit_pos += 1;
        Ok(((self.data[byte_pos] >> bit_in_byte) & 1) as u32)
    }

    fn read_bits(&mut self, n: usize) -> Result<u32, ProbeError> {
        let mut val = 0u32;
        for _ in 0..n {
            val = (val << 1) | self.read_bit()?;
        }
        Ok(val)
    }

    fn read_ue(&mut self) -> Result<u32, ProbeError> {
        let mut leading_zeros = 0usize;
        while self.read_bit()? == 0 {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return Err(ProbeError::malformed("annexb", "invalid exp-golomb code"));
            }
        }
        if leading_zeros == 0 {
            return Ok(0);
        }
        let rest = self.read_bits(leading_zeros)?;
        Ok((1 << leading_zeros) - 1 + rest)
    }

    fn read_se(&mut self) -> Result<i32, ProbeError> {
        let ue = self.read_ue()?;
        if ue % 2 == 1 {
            Ok(ue.div_ceil(2) as i32)
        } else {
            Ok(-((ue / 2) as i32))
        }
    }

    fn skip_bits(&mut self, n: usize) -> Result<(), ProbeError> {
        if self.bit_pos + n > self.data.len() * 8 {
            return Err(ProbeError::Truncated("bitstream"));
        }
        self.bit_pos += n;
        Ok(())
    }
}

/// Parse an H.264 SPS NAL unit (NAL header included).
pub fn parse_h264_sps(nalu: &[u8]) -> Result<VideoInfo, ProbeError> {
    if nalu.len() < 4 {
        return Err(ProbeError::Truncated("h264 sps"));
    }
    let rbsp = unescape_rbsp(nalu);
    // rbsp[0] is NAL header (forbidden_zero_bit, nal_ref_idc, nal_unit_type)
    if rbsp.len() < 4 {
        return Err(ProbeError::Truncated("h264 sps"));
    }

    let profile_idc = rbsp[1];
    let _constraint_set_flags = rbsp[2];
    let level_idc = rbsp[3];

    let mut reader = BitReader::new(&rbsp[4..]);

    let _seq_parameter_set_id = reader.read_ue()?;

    let mut chroma_format_idc = 1;
    let mut bit_depth_luma = 8;
    let mut bit_depth_chroma = 8;

    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = reader.read_ue()?;
        if chroma_format_idc == 3 {
            let _separate_colour_plane_flag = reader.read_bit()?;
        }
        bit_depth_luma = 8 + reader.read_ue()?;
        bit_depth_chroma = 8 + reader.read_ue()?;
        let _qpprime_y_zero_transform_bypass_flag = reader.read_bit()?;
        let seq_scaling_matrix_present_flag = reader.read_bit()?;
        if seq_scaling_matrix_present_flag != 0 {
            let num_scaling_lists = if chroma_format_idc != 3 { 8 } else { 12 };
            for i in 0..num_scaling_lists {
                let seq_scaling_list_present_flag = reader.read_bit()?;
                if seq_scaling_list_present_flag != 0 {
                    let size_of_scaling_list = if i < 6 { 16 } else { 64 };
                    let mut last_scale = 8i32;
                    let mut next_scale = 8i32;
                    for _ in 0..size_of_scaling_list {
                        if next_scale != 0 {
                            let delta_scale = reader.read_se()?;
                            next_scale = (last_scale + delta_scale + 256) % 256;
                        }
                        last_scale = if next_scale == 0 {
                            last_scale
                        } else {
                            next_scale
                        };
                    }
                }
            }
        }
    }

    let _log2_max_frame_num_minus4 = reader.read_ue()?;
    let pic_order_cnt_type = reader.read_ue()?;
    if pic_order_cnt_type == 0 {
        let _log2_max_pic_order_cnt_lsb_minus4 = reader.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        let _delta_pic_order_always_zero_flag = reader.read_bit()?;
        let _offset_for_non_ref_pic = reader.read_se()?;
        let _offset_for_top_to_bottom_field = reader.read_se()?;
        let num_ref_frames_in_pic_order_cnt_cycle = reader.read_ue()?;
        for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
            let _offset = reader.read_se()?;
        }
    }

    let _max_num_ref_frames = reader.read_ue()?;
    let _gaps_in_frame_num_value_allowed_flag = reader.read_bit()?;
    let pic_width_in_mbs_minus1 = reader.read_ue()?;
    let pic_height_in_map_units_minus1 = reader.read_ue()?;
    let frame_mbs_only_flag = reader.read_bit()?;
    if frame_mbs_only_flag == 0 {
        let _mb_adaptive_frame_field_flag = reader.read_bit()?;
    }
    let _direct_8x8_inference_flag = reader.read_bit()?;

    let frame_cropping_flag = reader.read_bit()?;
    let mut crop_left = 0u32;
    let mut crop_right = 0u32;
    let mut crop_top = 0u32;
    let mut crop_bottom = 0u32;
    if frame_cropping_flag != 0 {
        crop_left = reader.read_ue()?;
        crop_right = reader.read_ue()?;
        crop_top = reader.read_ue()?;
        crop_bottom = reader.read_ue()?;
    }

    // Resolution calculation
    let raw_width = (pic_width_in_mbs_minus1 + 1) * 16;
    let raw_height = (pic_height_in_map_units_minus1 + 1) * 16 * (2 - frame_mbs_only_flag);

    let (crop_unit_x, crop_unit_y) = match chroma_format_idc {
        0 => (1, 2 - frame_mbs_only_flag),
        1 => (2, 2 * (2 - frame_mbs_only_flag)), // 4:2:0
        2 => (2, 2 - frame_mbs_only_flag),       // 4:2:2
        3 => (1, 2 - frame_mbs_only_flag),       // 4:4:4
        _ => (1, 1),
    };

    let width = raw_width.saturating_sub((crop_left + crop_right) * crop_unit_x);
    let height = raw_height.saturating_sub((crop_top + crop_bottom) * crop_unit_y);

    let profile_name = match profile_idc {
        66 => "Baseline",
        77 => "Main",
        88 => "Extended",
        100 => "High",
        110 => "High 10",
        122 => "High 4:2:2",
        144 => "High 4:4:4",
        _ => "Unknown",
    };

    let ten_bit = bit_depth_luma > 8
        || bit_depth_chroma > 8
        || profile_idc == 110
        || profile_idc == 122
        || profile_idc == 144;

    let mut info = VideoInfo::new(VideoCodec::H264);
    info.width = Some(width);
    info.height = Some(height);
    info.profile = Some(profile_name.to_string());
    info.level = Some(level_idc);
    info.ten_bit = Some(ten_bit);
    Ok(info)
}

/// Parse an H.265 (HEVC) SPS NAL unit (NAL header included).
pub fn parse_hevc_sps(nalu: &[u8]) -> Result<VideoInfo, ProbeError> {
    if nalu.len() < 3 {
        return Err(ProbeError::Truncated("hevc sps"));
    }
    let rbsp = unescape_rbsp(nalu);
    // rbsp[0..2] is HEVC NAL unit header (2 bytes)
    if rbsp.len() < 3 {
        return Err(ProbeError::Truncated("hevc sps"));
    }

    let mut reader = BitReader::new(&rbsp[2..]);

    let _sps_video_parameter_set_id = reader.read_bits(4)?;
    let sps_max_sub_layers_minus1 = reader.read_bits(3)?;
    let _sps_temporal_id_nesting_flag = reader.read_bit()?;

    // profile_tier_level(1, sps_max_sub_layers_minus1)
    let _general_profile_space = reader.read_bits(2)?;
    let _general_tier_flag = reader.read_bit()?;
    let general_profile_idc = reader.read_bits(5)?;
    let _general_profile_compatibility_flags = reader.read_bits(32)?;

    // general constraint flags: 48 bits (6 bytes)
    reader.skip_bits(48)?;

    let general_level_idc = reader.read_bits(8)? as u8;

    let mut sub_layer_profile_present_flag = [false; 8];
    let mut sub_layer_level_present_flag = [false; 8];
    for i in 0..sps_max_sub_layers_minus1 as usize {
        sub_layer_profile_present_flag[i] = reader.read_bit()? != 0;
        sub_layer_level_present_flag[i] = reader.read_bit()? != 0;
    }
    if sps_max_sub_layers_minus1 > 0 {
        for _ in sps_max_sub_layers_minus1..8 {
            let _reserved_zero_2bits = reader.read_bits(2)?;
        }
    }
    for i in 0..sps_max_sub_layers_minus1 as usize {
        if sub_layer_profile_present_flag[i] {
            reader.skip_bits(2 + 1 + 5 + 32 + 48)?;
        }
        if sub_layer_level_present_flag[i] {
            reader.skip_bits(8)?;
        }
    }

    let _sps_seq_parameter_set_id = reader.read_ue()?;
    let chroma_format_idc = reader.read_ue()?;
    if chroma_format_idc == 3 {
        let _separate_colour_plane_flag = reader.read_bit()?;
    }
    let pic_width_in_luma_samples = reader.read_ue()?;
    let pic_height_in_luma_samples = reader.read_ue()?;

    let conformance_window_flag = reader.read_bit()?;
    let mut conf_win_left_offset = 0u32;
    let mut conf_win_right_offset = 0u32;
    let mut conf_win_top_offset = 0u32;
    let mut conf_win_bottom_offset = 0u32;

    if conformance_window_flag != 0 {
        conf_win_left_offset = reader.read_ue()?;
        conf_win_right_offset = reader.read_ue()?;
        conf_win_top_offset = reader.read_ue()?;
        conf_win_bottom_offset = reader.read_ue()?;
    }

    let bit_depth_luma_minus8 = reader.read_ue()?;
    let bit_depth_chroma_minus8 = reader.read_ue()?;
    let bit_depth_luma = 8 + bit_depth_luma_minus8;
    let bit_depth_chroma = 8 + bit_depth_chroma_minus8;

    let sub_width_c = if chroma_format_idc == 1 || chroma_format_idc == 2 {
        2
    } else {
        1
    };
    let sub_height_c = if chroma_format_idc == 1 { 2 } else { 1 };

    let width = pic_width_in_luma_samples
        .saturating_sub((conf_win_left_offset + conf_win_right_offset) * sub_width_c);
    let height = pic_height_in_luma_samples
        .saturating_sub((conf_win_top_offset + conf_win_bottom_offset) * sub_height_c);

    let ten_bit = bit_depth_luma > 8
        || bit_depth_chroma > 8
        || general_profile_idc == 2
        || general_profile_idc == 4
        || general_profile_idc == 5;

    let mut info = VideoInfo::new(VideoCodec::Hevc);
    info.width = Some(width);
    info.height = Some(height);
    info.profile = Some("HEVC".to_string());
    info.level = Some(general_level_idc);
    info.ten_bit = Some(ten_bit);
    Ok(info)
}
