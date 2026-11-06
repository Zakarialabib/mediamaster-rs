//! Image preprocessing for palette extraction.
//!
//! Includes:
//! - Uniform border trimming (up to <= 22% of dimension if borders are uniform)
//! - Area-box downsampling to <= 112x112
//! - Dropping alpha < 128 and CIE L* < 1.5 (near-black)

use crate::palette::color::{Lab, Rgba};
use alloc::vec::Vec;

/// Preprocesses raw RGBA image pixels into a downsampled, filtered slice of opaque pixels.
///
/// Steps:
/// 1. Border trimming (detect uniform borders on 4 sides up to max 22% on each side).
/// 2. Area-box downsampling preserving aspect ratio so `max(width, height) <= 112`.
/// 3. Filter out pixels where `alpha < 128` or `CIE L* < 1.5`.
pub fn preprocess_image(pixels: &[Rgba], width: usize, height: usize) -> Vec<Rgba> {
    if pixels.is_empty() || width == 0 || height == 0 {
        return Vec::new();
    }

    // Step 1: Border trimming (<= 22% on any edge if rows/cols are uniform)
    let max_trim_x = ((width as f64) * 0.22) as usize;
    let max_trim_y = ((height as f64) * 0.22) as usize;

    let is_row_uniform = |y: usize| -> bool {
        if width == 0 {
            return true;
        }
        let first = pixels[y * width];
        for x in 1..width {
            let p = pixels[y * width + x];
            if p != first {
                return false;
            }
        }
        true
    };

    let is_col_uniform = |x: usize| -> bool {
        if height == 0 {
            return true;
        }
        let first = pixels[x];
        for y in 1..height {
            let p = pixels[y * width + x];
            if p != first {
                return false;
            }
        }
        true
    };

    let mut trim_top = 0;
    while trim_top < max_trim_y && is_row_uniform(trim_top) {
        trim_top += 1;
    }

    let mut trim_bottom = 0;
    while trim_bottom < max_trim_y && is_row_uniform(height - 1 - trim_bottom) {
        trim_bottom += 1;
    }

    let mut trim_left = 0;
    while trim_left < max_trim_x && is_col_uniform(trim_left) {
        trim_left += 1;
    }

    let mut trim_right = 0;
    while trim_right < max_trim_x && is_col_uniform(width - 1 - trim_right) {
        trim_right += 1;
    }

    let cropped_x0 = trim_left;
    let cropped_x1 = width.saturating_sub(trim_right).max(cropped_x0 + 1);
    let cropped_y0 = trim_top;
    let cropped_y1 = height.saturating_sub(trim_bottom).max(cropped_y0 + 1);

    let cropped_w = cropped_x1 - cropped_x0;
    let cropped_h = cropped_y1 - cropped_y0;

    // Step 2: Area-box downsample to <= 112x112
    const TARGET_MAX: usize = 112;
    let (target_w, target_h) = if cropped_w <= TARGET_MAX && cropped_h <= TARGET_MAX {
        (cropped_w, cropped_h)
    } else {
        let max_dim = cropped_w.max(cropped_h);
        let scale = (TARGET_MAX as f64) / (max_dim as f64);
        (
            libm::round((cropped_w as f64) * scale).max(1.0) as usize,
            libm::round((cropped_h as f64) * scale).max(1.0) as usize,
        )
    };

    let mut downsampled = Vec::with_capacity(target_w * target_h);

    let x_scale = (cropped_w as f64) / (target_w as f64);
    let y_scale = (cropped_h as f64) / (target_h as f64);

    for ty in 0..target_h {
        let sy0 = cropped_y0 + ((ty as f64) * y_scale) as usize;
        let sy1 = (cropped_y0 + libm::ceil(((ty + 1) as f64) * y_scale) as usize).min(cropped_y1);
        let sy1 = sy1.max(sy0 + 1);

        for tx in 0..target_w {
            let sx0 = cropped_x0 + ((tx as f64) * x_scale) as usize;
            let sx1 =
                (cropped_x0 + libm::ceil(((tx + 1) as f64) * x_scale) as usize).min(cropped_x1);
            let sx1 = sx1.max(sx0 + 1);

            let mut sum_r = 0u64;
            let mut sum_g = 0u64;
            let mut sum_b = 0u64;
            let mut sum_a = 0u64;
            let mut count = 0u64;

            for y in sy0..sy1 {
                for x in sx0..sx1 {
                    let p = pixels[y * width + x];
                    sum_r += p.r as u64;
                    sum_g += p.g as u64;
                    sum_b += p.b as u64;
                    sum_a += p.a as u64;
                    count += 1;
                }
            }

            if let Some(r) = sum_r.checked_div(count) {
                let g = sum_g.checked_div(count).unwrap_or(0);
                let b = sum_b.checked_div(count).unwrap_or(0);
                let a = sum_a.checked_div(count).unwrap_or(0);
                downsampled.push(Rgba::new(r as u8, g as u8, b as u8, a as u8));
            }
        }
    }

    // Step 3: Drop alpha < 128 and CIE L* < 1.5
    let mut filtered = Vec::with_capacity(downsampled.len());
    for p in downsampled {
        if p.a < 128 {
            continue;
        }
        let lab = Lab::from_rgba(p);
        if lab.l < 1.5 {
            continue;
        }
        filtered.push(p);
    }

    filtered
}
