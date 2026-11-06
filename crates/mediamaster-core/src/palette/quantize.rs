//! Celebi quantization in CIE L*a*b* space.
//!
//! Celebi quantization combines:
//! 1. Wu color quantizer (recursive variance minimization in RGB) to generate initial cluster seeds.
//! 2. Wsmeans (Weighted Square Means K-Means) in CIE L*a*b* with triangle inequality acceleration.

use crate::palette::color::{Lab, Rgba};
use alloc::vec;
use alloc::vec::Vec;

const INDEX_BITS: usize = 5;
const SIDE_LENGTH: usize = (1 << INDEX_BITS) + 1; // 33
const TOTAL_SIZE: usize = SIDE_LENGTH * SIDE_LENGTH * SIDE_LENGTH; // 35937

const MAX_KMEANS_ITERATIONS: usize = 10;
const MIN_MOVEMENT_DISTANCE: f64 = 3.0;

/// Deterministic 48-bit Linear Congruential Generator matching java.util.Random(0x42688).
#[derive(Clone, Debug)]
struct SeededRandom {
    lo: u32,
    hi: u32,
}

impl SeededRandom {
    fn new(seed: u64) -> Self {
        let lo = ((seed & 0xffffff) ^ 0xece66d) as u32;
        let hi = (((seed >> 24) & 0xffffff) ^ 0x5de) as u32;
        Self { lo, hi }
    }

    fn next_bits(&mut self, bits: u32) -> u32 {
        let lo_product = (self.lo as u64) * 0xece66d + 0xb;
        let carry = (lo_product / 0x1000000) as u32;
        let next_lo = (lo_product % 0x1000000) as u32;
        let hi_product = (self.hi as u64) * 0xece66d + (self.lo as u64) * 0x5de + (carry as u64);
        self.lo = next_lo;
        self.hi = (hi_product & 0xffffff) as u32;
        let shift = 48 - bits;
        (self.hi << (24 - shift)) | (self.lo >> shift)
    }

    fn next_int(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        if (bound & (bound.wrapping_neg())) == bound {
            return (((bound as u64) * (self.next_bits(31) as u64)) / 0x80000000) as usize;
        }
        loop {
            let bits = self.next_bits(31);
            let val = (bits as usize) % bound;
            if (bits as u64)
                .wrapping_sub(val as u64)
                .wrapping_add((bound as u64) - 1)
                < 0x80000000
            {
                return val;
            }
        }
    }

    fn next_f64(&mut self) -> f64 {
        let val = ((self.next_bits(26) as u64) * 0x8000000) + (self.next_bits(27) as u64);
        (val as f64) / ((1u64 << 53) as f64)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Box {
    r0: usize,
    r1: usize,
    g0: usize,
    g1: usize,
    b0: usize,
    b1: usize,
    vol: usize,
}

#[inline]
fn get_index(r: usize, g: usize, b: usize) -> usize {
    (r << (INDEX_BITS * 2)) + (r << (INDEX_BITS + 1)) + r + (g << INDEX_BITS) + g + b
}

/// Wu quantization algorithm to generate initial cluster seeds.
pub fn quantize_wu(pixels: &[Rgba], max_colors: usize) -> Vec<Rgba> {
    if pixels.is_empty() || max_colors == 0 {
        return Vec::new();
    }

    let mut weights = vec![0u32; TOTAL_SIZE];
    let mut moments_r = vec![0u32; TOTAL_SIZE];
    let mut moments_g = vec![0u32; TOTAL_SIZE];
    let mut moments_b = vec![0u32; TOTAL_SIZE];
    let mut moments = vec![0.0f64; TOTAL_SIZE];

    let bits_to_remove = 8 - INDEX_BITS;

    for &p in pixels {
        let i_r = ((p.r as usize) >> bits_to_remove) + 1;
        let i_g = ((p.g as usize) >> bits_to_remove) + 1;
        let i_b = ((p.b as usize) >> bits_to_remove) + 1;
        let idx = get_index(i_r, i_g, i_b);

        weights[idx] += 1;
        moments_r[idx] += p.r as u32;
        moments_g[idx] += p.g as u32;
        moments_b[idx] += p.b as u32;
        moments[idx] +=
            (p.r as f64) * (p.r as f64) + (p.g as f64) * (p.g as f64) + (p.b as f64) * (p.b as f64);
    }

    // Compute cumulative 3D moments
    for r in 1..SIDE_LENGTH {
        let mut area = [0u32; SIDE_LENGTH];
        let mut area_r = [0u32; SIDE_LENGTH];
        let mut area_g = [0u32; SIDE_LENGTH];
        let mut area_b = [0u32; SIDE_LENGTH];
        let mut area2 = [0.0f64; SIDE_LENGTH];

        for g in 1..SIDE_LENGTH {
            let mut line = 0u32;
            let mut line_r = 0u32;
            let mut line_g = 0u32;
            let mut line_b = 0u32;
            let mut line2 = 0.0f64;

            for b in 1..SIDE_LENGTH {
                let idx = get_index(r, g, b);
                line += weights[idx];
                line_r += moments_r[idx];
                line_g += moments_g[idx];
                line_b += moments_b[idx];
                line2 += moments[idx];

                area[b] += line;
                area_r[b] += line_r;
                area_g[b] += line_g;
                area_b[b] += line_b;
                area2[b] += line2;

                let prev_idx = get_index(r - 1, g, b);
                weights[idx] = weights[prev_idx] + area[b];
                moments_r[idx] = moments_r[prev_idx] + area_r[b];
                moments_g[idx] = moments_g[prev_idx] + area_g[b];
                moments_b[idx] = moments_b[prev_idx] + area_b[b];
                moments[idx] = moments[prev_idx] + area2[b];
            }
        }
    }

    let mut cubes = vec![Box::default(); max_colors];
    let mut volume_variance = vec![0.0f64; max_colors];

    cubes[0] = Box {
        r0: 0,
        r1: SIDE_LENGTH - 1,
        g0: 0,
        g1: SIDE_LENGTH - 1,
        b0: 0,
        b1: SIDE_LENGTH - 1,
        vol: 0,
    };

    let volume_u32 = |cube: &Box, moment: &[u32]| -> i64 {
        (moment[get_index(cube.r1, cube.g1, cube.b1)] as i64)
            - (moment[get_index(cube.r1, cube.g1, cube.b0)] as i64)
            - (moment[get_index(cube.r1, cube.g0, cube.b1)] as i64)
            + (moment[get_index(cube.r1, cube.g0, cube.b0)] as i64)
            - (moment[get_index(cube.r0, cube.g1, cube.b1)] as i64)
            + (moment[get_index(cube.r0, cube.g1, cube.b0)] as i64)
            + (moment[get_index(cube.r0, cube.g0, cube.b1)] as i64)
            - (moment[get_index(cube.r0, cube.g0, cube.b0)] as i64)
    };

    let volume_f64 = |cube: &Box, moment: &[f64]| -> f64 {
        moment[get_index(cube.r1, cube.g1, cube.b1)]
            - moment[get_index(cube.r1, cube.g1, cube.b0)]
            - moment[get_index(cube.r1, cube.g0, cube.b1)]
            + moment[get_index(cube.r1, cube.g0, cube.b0)]
            - moment[get_index(cube.r0, cube.g1, cube.b1)]
            + moment[get_index(cube.r0, cube.g1, cube.b0)]
            + moment[get_index(cube.r0, cube.g0, cube.b1)]
            - moment[get_index(cube.r0, cube.g0, cube.b0)]
    };

    let variance = |cube: &Box| -> f64 {
        let dr = volume_u32(cube, &moments_r) as f64;
        let dg = volume_u32(cube, &moments_g) as f64;
        let db = volume_u32(cube, &moments_b) as f64;
        let xx = volume_f64(cube, &moments);
        let hypotenuse = dr * dr + dg * dg + db * db;
        let vol = volume_u32(cube, &weights) as f64;
        if vol <= 0.0 {
            0.0
        } else {
            xx - hypotenuse / vol
        }
    };

    let bottom = |cube: &Box, dir: u8, moment: &[u32]| -> i64 {
        match dir {
            0 => {
                -(moment[get_index(cube.r0, cube.g1, cube.b1)] as i64)
                    + (moment[get_index(cube.r0, cube.g1, cube.b0)] as i64)
                    + (moment[get_index(cube.r0, cube.g0, cube.b1)] as i64)
                    - (moment[get_index(cube.r0, cube.g0, cube.b0)] as i64)
            }
            1 => {
                -(moment[get_index(cube.r1, cube.g0, cube.b1)] as i64)
                    + (moment[get_index(cube.r1, cube.g0, cube.b0)] as i64)
                    + (moment[get_index(cube.r0, cube.g0, cube.b1)] as i64)
                    - (moment[get_index(cube.r0, cube.g0, cube.b0)] as i64)
            }
            _ => {
                -(moment[get_index(cube.r1, cube.g1, cube.b0)] as i64)
                    + (moment[get_index(cube.r1, cube.g0, cube.b0)] as i64)
                    + (moment[get_index(cube.r0, cube.g1, cube.b0)] as i64)
                    - (moment[get_index(cube.r0, cube.g0, cube.b0)] as i64)
            }
        }
    };

    let top = |cube: &Box, dir: u8, pos: usize, moment: &[u32]| -> i64 {
        match dir {
            0 => {
                (moment[get_index(pos, cube.g1, cube.b1)] as i64)
                    - (moment[get_index(pos, cube.g1, cube.b0)] as i64)
                    - (moment[get_index(pos, cube.g0, cube.b1)] as i64)
                    + (moment[get_index(pos, cube.g0, cube.b0)] as i64)
            }
            1 => {
                (moment[get_index(cube.r1, pos, cube.b1)] as i64)
                    - (moment[get_index(cube.r1, pos, cube.b0)] as i64)
                    - (moment[get_index(cube.r0, pos, cube.b1)] as i64)
                    + (moment[get_index(cube.r0, pos, cube.b0)] as i64)
            }
            _ => {
                (moment[get_index(cube.r1, cube.g1, pos)] as i64)
                    - (moment[get_index(cube.r1, cube.g0, pos)] as i64)
                    - (moment[get_index(cube.r0, cube.g1, pos)] as i64)
                    + (moment[get_index(cube.r0, cube.g0, pos)] as i64)
            }
        }
    };

    let maximize = |cube: &Box,
                    dir: u8,
                    first: usize,
                    last: usize,
                    whole_r: i64,
                    whole_g: i64,
                    whole_b: i64,
                    whole_w: i64|
     -> (i32, f64) {
        let b_r = bottom(cube, dir, &moments_r);
        let b_g = bottom(cube, dir, &moments_g);
        let b_b = bottom(cube, dir, &moments_b);
        let b_w = bottom(cube, dir, &weights);

        let mut max_val = 0.0f64;
        let mut cut_loc = -1i32;

        for i in first..last {
            let mut h_r = b_r + top(cube, dir, i, &moments_r);
            let mut h_g = b_g + top(cube, dir, i, &moments_g);
            let mut h_b = b_b + top(cube, dir, i, &moments_b);
            let mut h_w = b_w + top(cube, dir, i, &weights);

            if h_w == 0 {
                continue;
            }

            let mut temp = ((h_r * h_r + h_g * h_g + h_b * h_b) as f64) / (h_w as f64);

            h_r = whole_r - h_r;
            h_g = whole_g - h_g;
            h_b = whole_b - h_b;
            h_w = whole_w - h_w;

            if h_w == 0 {
                continue;
            }

            temp += ((h_r * h_r + h_g * h_g + h_b * h_b) as f64) / (h_w as f64);

            if temp > max_val {
                max_val = temp;
                cut_loc = i as i32;
            }
        }

        (cut_loc, max_val)
    };

    let mut generated_count = max_colors;
    let mut next = 0;

    for i in 1..max_colors {
        let whole_r = volume_u32(&cubes[next], &moments_r);
        let whole_g = volume_u32(&cubes[next], &moments_g);
        let whole_b = volume_u32(&cubes[next], &moments_b);
        let whole_w = volume_u32(&cubes[next], &weights);

        let (cut_r, max_r) = maximize(
            &cubes[next],
            0,
            cubes[next].r0 + 1,
            cubes[next].r1,
            whole_r,
            whole_g,
            whole_b,
            whole_w,
        );
        let (cut_g, max_g) = maximize(
            &cubes[next],
            1,
            cubes[next].g0 + 1,
            cubes[next].g1,
            whole_r,
            whole_g,
            whole_b,
            whole_w,
        );
        let (cut_b, max_b) = maximize(
            &cubes[next],
            2,
            cubes[next].b0 + 1,
            cubes[next].b1,
            whole_r,
            whole_g,
            whole_b,
            whole_w,
        );

        let (dir, cut_loc) = if max_r >= max_g && max_r >= max_b {
            (0u8, cut_r)
        } else if max_g >= max_r && max_g >= max_b {
            (1u8, cut_g)
        } else {
            (2u8, cut_b)
        };

        if cut_loc >= 0 {
            cubes[i] = cubes[next];
            match dir {
                0 => {
                    cubes[next].r1 = cut_loc as usize;
                    cubes[i].r0 = cut_loc as usize;
                }
                1 => {
                    cubes[next].g1 = cut_loc as usize;
                    cubes[i].g0 = cut_loc as usize;
                }
                _ => {
                    cubes[next].b1 = cut_loc as usize;
                    cubes[i].b0 = cut_loc as usize;
                }
            }

            cubes[next].vol = (cubes[next].r1 - cubes[next].r0)
                * (cubes[next].g1 - cubes[next].g0)
                * (cubes[next].b1 - cubes[next].b0);
            cubes[i].vol = (cubes[i].r1 - cubes[i].r0)
                * (cubes[i].g1 - cubes[i].g0)
                * (cubes[i].b1 - cubes[i].b0);

            volume_variance[next] = if cubes[next].vol > 1 {
                variance(&cubes[next])
            } else {
                0.0
            };
            volume_variance[i] = if cubes[i].vol > 1 {
                variance(&cubes[i])
            } else {
                0.0
            };
        } else {
            volume_variance[next] = 0.0;
        }

        next = 0;
        let mut temp = volume_variance[0];
        for (j, &var) in volume_variance.iter().enumerate().take(i + 1).skip(1) {
            if var > temp {
                temp = var;
                next = j;
            }
        }
        if temp <= 0.0 {
            generated_count = i + 1;
            break;
        }
    }

    let mut results = Vec::new();
    for cube in cubes.iter().take(generated_count) {
        let weight = volume_u32(cube, &weights);
        if weight > 0 {
            let r = (volume_u32(cube, &moments_r) / weight).clamp(0, 255) as u8;
            let g = (volume_u32(cube, &moments_g) / weight).clamp(0, 255) as u8;
            let b = (volume_u32(cube, &moments_b) / weight).clamp(0, 255) as u8;
            results.push(Rgba::rgb(r, g, b));
        }
    }

    results
}

/// Quantized color with associated pixel count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuantizedCluster {
    pub color: Rgba,
    pub population: usize,
}

/// Celebi quantization (Wu seeds + Wsmeans clustering in CIE L*a*b* space).
pub fn quantize_celebi(pixels: &[Rgba], max_colors: usize) -> Vec<QuantizedCluster> {
    if pixels.is_empty() || max_colors == 0 {
        return Vec::new();
    }
    let max_colors = max_colors.min(128);

    // Wu initial seeds
    let wu_seeds = quantize_wu(pixels, max_colors);

    // Build deduplicated pixel points
    let mut unique_pixels: Vec<Rgba> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut points: Vec<Lab> = Vec::new();

    for &p in pixels {
        let p_opaque = Rgba::rgb(p.r, p.g, p.b);
        if let Some(pos) = unique_pixels.iter().position(|&x| x == p_opaque) {
            counts[pos] += 1;
        } else {
            unique_pixels.push(p_opaque);
            counts.push(1);
            points.push(Lab::from_rgba(p_opaque));
        }
    }

    let point_count = unique_pixels.len();
    let cluster_count = max_colors.min(point_count).min(wu_seeds.len().max(1));

    let mut clusters: Vec<Lab> = wu_seeds
        .into_iter()
        .take(cluster_count)
        .map(Lab::from_rgba)
        .collect();

    let mut random = SeededRandom::new(0x42688);

    // If more clusters are needed
    while clusters.len() < cluster_count {
        let l = random.next_f64() * 100.0;
        let a = random.next_f64() * 200.0 - 100.0;
        let b = random.next_f64() * 200.0 - 100.0;
        clusters.push(Lab { l, a, b });
    }

    let mut cluster_indices: Vec<usize> = (0..point_count)
        .map(|_| random.next_int(cluster_count))
        .collect();

    let mut dist_matrix = vec![vec![0.0f64; cluster_count]; cluster_count];

    #[inline]
    fn lab_dist_sq(p1: &Lab, p2: &Lab) -> f64 {
        let dl = p1.l - p2.l;
        let da = p1.a - p2.a;
        let db = p1.b - p2.b;
        dl * dl + da * da + db * db
    }

    let mut pixel_count_sums = vec![0usize; cluster_count];

    for iteration in 0..MAX_KMEANS_ITERATIONS {
        for i in 0..cluster_count {
            for j in (i + 1)..cluster_count {
                let dist = lab_dist_sq(&clusters[i], &clusters[j]);
                dist_matrix[i][j] = dist;
                dist_matrix[j][i] = dist;
            }
        }

        let mut points_moved = 0;
        for i in 0..point_count {
            let point = &points[i];
            let prev_cluster_idx = cluster_indices[i];
            let prev_cluster = &clusters[prev_cluster_idx];
            let prev_dist = lab_dist_sq(point, prev_cluster);
            let mut min_dist = prev_dist;
            let mut new_cluster_idx = None;

            for j in 0..cluster_count {
                if dist_matrix[prev_cluster_idx][j] >= 4.0 * prev_dist {
                    continue;
                }
                let dist = lab_dist_sq(point, &clusters[j]);
                if dist < min_dist {
                    min_dist = dist;
                    new_cluster_idx = Some(j);
                }
            }

            if let Some(new_idx) = new_cluster_idx {
                let dist_change = libm::fabs(libm::sqrt(min_dist) - libm::sqrt(prev_dist));
                if dist_change > MIN_MOVEMENT_DISTANCE {
                    points_moved += 1;
                    cluster_indices[i] = new_idx;
                }
            }
        }

        if points_moved == 0 && iteration != 0 {
            break;
        }

        // Recompute cluster centroids
        let mut sum_l = vec![0.0f64; cluster_count];
        let mut sum_a = vec![0.0f64; cluster_count];
        let mut sum_b = vec![0.0f64; cluster_count];
        pixel_count_sums.fill(0);

        for i in 0..point_count {
            let idx = cluster_indices[i];
            let count = counts[i] as f64;
            pixel_count_sums[idx] += counts[i];
            sum_l[idx] += points[i].l * count;
            sum_a[idx] += points[i].a * count;
            sum_b[idx] += points[i].b * count;
        }

        for i in 0..cluster_count {
            let count = pixel_count_sums[i] as f64;
            if count > 0.0 {
                clusters[i] = Lab {
                    l: sum_l[i] / count,
                    a: sum_a[i] / count,
                    b: sum_b[i] / count,
                };
            }
        }
    }

    let mut result = Vec::new();
    for (i, cluster) in clusters.iter().enumerate().take(cluster_count) {
        let count = pixel_count_sums[i];
        if count > 0 {
            result.push(QuantizedCluster {
                color: cluster.to_rgba(),
                population: count,
            });
        }
    }

    result
}
