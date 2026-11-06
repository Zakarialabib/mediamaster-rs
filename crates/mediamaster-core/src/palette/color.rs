//! Color representations and conversions for signage palette extraction.
//!
//! Includes sRGB, CIE L*a*b*, Oklab, and HCT (CAM16 hue & chroma + CIE L* tone).

use core::f64::consts::PI;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// 8-bit per channel RGBA color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    #[inline]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    #[inline]
    pub const fn to_argb_u32(self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    #[inline]
    pub const fn from_argb_u32(val: u32) -> Self {
        Self {
            a: ((val >> 24) & 0xff) as u8,
            r: ((val >> 16) & 0xff) as u8,
            g: ((val >> 8) & 0xff) as u8,
            b: (val & 0xff) as u8,
        }
    }

    /// Converts to hex string `#rrggbb`
    pub fn to_hex(&self) -> alloc::string::String {
        alloc::format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Relative luminance according to WCAG 2.1 (using sRGB D65).
    pub fn relative_luminance(&self) -> f64 {
        let r = srgb_to_linear(self.r);
        let g = srgb_to_linear(self.g);
        let b = srgb_to_linear(self.b);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// Contrast ratio between two colors according to WCAG 2.1.
    pub fn contrast_ratio(&self, other: &Self) -> f64 {
        let l1 = self.relative_luminance();
        let l2 = other.relative_luminance();
        let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Checks if contrast meets WCAG AA for normal text (>= 4.5:1).
    pub fn is_wcag_aa(&self, background: &Self) -> bool {
        self.contrast_ratio(background) >= 4.5
    }

    /// Checks if contrast meets WCAG AAA for normal text (>= 7.0:1).
    pub fn is_wcag_aaa(&self, background: &Self) -> bool {
        self.contrast_ratio(background) >= 7.0
    }
}

/// CIE 1976 L*a*b* color.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct Lab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

impl Lab {
    pub fn from_rgba(rgba: Rgba) -> Self {
        let r = srgb_to_linear(rgba.r) * 100.0;
        let g = srgb_to_linear(rgba.g) * 100.0;
        let b = srgb_to_linear(rgba.b) * 100.0;

        let x = 0.41233895 * r + 0.35762064 * g + 0.18051042 * b;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = 0.01932141 * r + 0.11916382 * g + 0.95034478 * b;

        let x_norm = x / 95.047;
        let y_norm = y / 100.0;
        let z_norm = z / 108.883;

        let fx = lab_f(x_norm);
        let fy = lab_f(y_norm);
        let fz = lab_f(z_norm);

        let l = 116.0 * fy - 16.0;
        let a_val = 500.0 * (fx - fy);
        let b_val = 200.0 * (fy - fz);

        Self {
            l,
            a: a_val,
            b: b_val,
        }
    }

    pub fn to_rgba(&self) -> Rgba {
        let fy = (self.l + 16.0) / 116.0;
        let fx = self.a / 500.0 + fy;
        let fz = fy - self.b / 200.0;

        let x_norm = lab_inv_f(fx);
        let y_norm = lab_inv_f(fy);
        let z_norm = lab_inv_f(fz);

        let x = x_norm * 95.047;
        let y = y_norm * 100.0;
        let z = z_norm * 108.883;

        let r_lin = 3.2413774792388685 * (x / 100.0)
            - 1.5376652402851851 * (y / 100.0)
            - 0.49885366846268053 * (z / 100.0);
        let g_lin = -0.9691452513005321 * (x / 100.0)
            + 1.8758853451067872 * (y / 100.0)
            + 0.04156585616912061 * (z / 100.0);
        let b_lin = 0.05562093689691305 * (x / 100.0) - 0.20395524564742123 * (y / 100.0)
            + 1.0571799111220335 * (z / 100.0);

        Rgba::new(
            linear_to_srgb(r_lin),
            linear_to_srgb(g_lin),
            linear_to_srgb(b_lin),
            255,
        )
    }

    /// Delta E 1976 Euclidean distance.
    #[inline]
    pub fn delta_e_76(&self, other: &Self) -> f64 {
        let dl = self.l - other.l;
        let da = self.a - other.a;
        let db = self.b - other.b;
        libm::sqrt(dl * dl + da * da + db * db)
    }
}

/// Oklab color representation.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

impl Oklab {
    pub fn from_rgba(rgba: Rgba) -> Self {
        let r = srgb_to_linear(rgba.r);
        let g = srgb_to_linear(rgba.g);
        let b = srgb_to_linear(rgba.b);

        let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
        let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
        let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

        let l_ = libm::cbrt(l);
        let m_ = libm::cbrt(m);
        let s_ = libm::cbrt(s);

        let ok_l = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
        let ok_a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
        let ok_b = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;

        Self {
            l: ok_l,
            a: ok_a,
            b: ok_b,
        }
    }

    pub fn to_rgba(&self) -> Rgba {
        let l_ = self.l + 0.3963377774 * self.a + 0.2158037573 * self.b;
        let m_ = self.l - 0.1055613458 * self.a - 0.0638541728 * self.b;
        let s_ = self.l - 0.0894841775 * self.a - 1.2914855480 * self.b;

        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;

        let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
        let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
        let b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;

        Rgba::new(linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b), 255)
    }

    /// Delta E in Oklab space (Euclidean distance).
    #[inline]
    pub fn delta_e(&self, other: &Self) -> f64 {
        let dl = self.l - other.l;
        let da = self.a - other.a;
        let db = self.b - other.b;
        libm::sqrt(dl * dl + da * da + db * db)
    }
}

/// HCT (Hue, Chroma, Tone) color representation.
/// - Hue: CAM16 hue in [0.0, 360.0)
/// - Chroma: CAM16 chroma >= 0.0
/// - Tone: CIE L* in [0.0, 100.0]
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct Hct {
    pub hue: f64,
    pub chroma: f64,
    pub tone: f64,
}

impl Hct {
    #[inline]
    pub fn new(hue: f64, chroma: f64, tone: f64) -> Self {
        Self {
            hue: sanitize_degrees(hue),
            chroma: chroma.max(0.0),
            tone: tone.clamp(0.0, 100.0),
        }
    }

    pub fn from_rgba(rgba: Rgba) -> Self {
        let cam = Cam16::from_rgba(rgba);
        let tone = Lab::from_rgba(rgba).l;
        Self {
            hue: cam.hue,
            chroma: cam.chroma,
            tone,
        }
    }

    pub fn to_rgba(&self) -> Rgba {
        solve_hct_to_rgba(self.hue, self.chroma, self.tone)
    }
}

#[inline]
pub fn srgb_to_linear(c: u8) -> f64 {
    let normalized = (c as f64) / 255.0;
    if normalized <= 0.04045 {
        normalized / 12.92
    } else {
        libm::pow((normalized + 0.055) / 1.055, 2.4)
    }
}

#[inline]
pub fn linear_to_srgb(c: f64) -> u8 {
    let clamped = c.clamp(0.0, 1.0);
    let val = if clamped <= 0.0031308 {
        clamped * 12.92
    } else {
        1.055 * libm::pow(clamped, 1.0 / 2.4) - 0.055
    };
    (libm::round(val * 255.0) as i64).clamp(0, 255) as u8
}

#[inline]
fn lab_f(t: f64) -> f64 {
    const E: f64 = 216.0 / 24389.0;
    const K: f64 = 24389.0 / 27.0;
    if t > E {
        libm::cbrt(t)
    } else {
        (K * t + 16.0) / 116.0
    }
}

#[inline]
fn lab_inv_f(ft: f64) -> f64 {
    const E: f64 = 216.0 / 24389.0;
    const K: f64 = 24389.0 / 27.0;
    let ft3 = ft * ft * ft;
    if ft3 > E {
        ft3
    } else {
        (116.0 * ft - 16.0) / K
    }
}

#[inline]
pub fn sanitize_degrees(deg: f64) -> f64 {
    let rem = libm::fmod(deg, 360.0);
    if rem < 0.0 {
        rem + 360.0
    } else {
        rem
    }
}

#[inline]
pub fn sanitize_radians(rad: f64) -> f64 {
    let rem = libm::fmod(rad + PI * 8.0, PI * 2.0);
    if rem < 0.0 {
        rem + PI * 2.0
    } else {
        rem
    }
}

/// CAM16 representation of color under default viewing conditions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cam16 {
    pub hue: f64,
    pub chroma: f64,
    pub j: f64,
}

impl Cam16 {
    pub fn from_rgba(rgba: Rgba) -> Self {
        let r_lin = srgb_to_linear(rgba.r) * 100.0;
        let g_lin = srgb_to_linear(rgba.g) * 100.0;
        let b_lin = srgb_to_linear(rgba.b) * 100.0;

        let x = 0.41233895 * r_lin + 0.35762064 * g_lin + 0.18051042 * b_lin;
        let y = 0.2126 * r_lin + 0.7152 * g_lin + 0.0722 * b_lin;
        let z = 0.01932141 * r_lin + 0.11916382 * g_lin + 0.95034478 * b_lin;

        let r_c = 0.401288 * x + 0.650173 * y - 0.051461 * z;
        let g_c = -0.250268 * x + 1.204414 * y + 0.045854 * z;
        let b_c = -0.002079 * x + 0.048952 * y + 0.953127 * z;

        const RGB_D: [f64; 3] = [1.021177769, 0.986307740, 0.933960497];
        let r_d = RGB_D[0] * r_c;
        let g_d = RGB_D[1] * g_c;
        let b_d = RGB_D[2] * b_c;

        const FL: f64 = 0.388481468;
        let r_af = libm::pow((FL * libm::fabs(r_d)) / 100.0, 0.42);
        let g_af = libm::pow((FL * libm::fabs(g_d)) / 100.0, 0.42);
        let b_af = libm::pow((FL * libm::fabs(b_d)) / 100.0, 0.42);

        let sign = |v: f64| {
            if v < 0.0 {
                -1.0
            } else if v > 0.0 {
                1.0
            } else {
                0.0
            }
        };
        let r_a = (sign(r_d) * 400.0 * r_af) / (r_af + 27.13);
        let g_a = (sign(g_d) * 400.0 * g_af) / (g_af + 27.13);
        let b_a = (sign(b_d) * 400.0 * b_af) / (b_af + 27.13);

        let a = (11.0 * r_a - 12.0 * g_a + b_a) / 11.0;
        let b = (r_a + g_a - 2.0 * b_a) / 9.0;
        let u = (20.0 * r_a + 20.0 * g_a + 21.0 * b_a) / 20.0;
        let p2 = (40.0 * r_a + 20.0 * g_a + b_a) / 20.0;

        let atan_deg = libm::atan2(b, a) * (180.0 / PI);
        let hue = sanitize_degrees(atan_deg);
        let hue_prime = if hue < 20.14 { hue + 360.0 } else { hue };
        let e_hue = 0.25 * (libm::cos((hue_prime * PI) / 180.0 + 2.0) + 3.8);

        const NBB: f64 = 1.016919255;
        const AW: f64 = 29.981000900;
        const C_SURROUND: f64 = 0.689999998;
        const Z: f64 = 1.909169555;
        const NC: f64 = 1.0;
        const NCB: f64 = 1.016919255;

        let ac = p2 * NBB;
        let j = 100.0 * libm::pow(ac / AW, C_SURROUND * Z);
        let p1 = (50000.0 / 13.0) * e_hue * NC * NCB;
        let t = (p1 * libm::hypot(a, b)) / (u + 0.305);
        let alpha = libm::pow(t, 0.9) * libm::pow(1.64 - libm::pow(0.29, 0.184186503), 0.73);
        let chroma = alpha * libm::sqrt(j / 100.0);

        Self { hue, chroma, j }
    }
}

// ----------------------------------------------------------------------------
// HCT Solver (Pure Rust, zero allocations)
// ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Vec3 {
    a: f64,
    b: f64,
    c: f64,
}

impl Vec3 {
    #[inline]
    const fn new(a: f64, b: f64, c: f64) -> Self {
        Self { a, b, c }
    }
}

const SCALED_DISCOUNT_FROM_LINRGB: [[f64; 3]; 3] = [
    [
        0.001200833568784504,
        0.002389694492170889,
        0.0002795742885861124,
    ],
    [
        0.0005891086651375999,
        0.0029785502573438758,
        0.0003270666104008398,
    ],
    [
        0.00010146692491640572,
        0.0005364214359186694,
        0.0032979401770712076,
    ],
];

const LINRGB_FROM_SCALED_DISCOUNT: [[f64; 3]; 3] = [
    [1373.2198709594231, -1100.4251190754821, -7.278681089101213],
    [-271.815969077903, 559.6580465940733, -32.46047482791194],
    [1.9622899599665666, -57.173814538844006, 308.7233197812385],
];

const K_Y_FROM_LINRGB: [f64; 3] = [0.2126, 0.7152, 0.0722];

#[inline]
fn matrix_multiply(vec: Vec3, mat: &[[f64; 3]; 3]) -> Vec3 {
    Vec3::new(
        vec.a * mat[0][0] + vec.b * mat[0][1] + vec.c * mat[0][2],
        vec.a * mat[1][0] + vec.b * mat[1][1] + vec.c * mat[1][2],
        vec.a * mat[2][0] + vec.b * mat[2][1] + vec.c * mat[2][2],
    )
}

#[inline]
fn true_delinearized(rgb_comp: f64) -> f64 {
    let norm = rgb_comp / 100.0;
    if norm <= 0.0031308 {
        norm * 12.92 * 255.0
    } else {
        (1.055 * libm::pow(norm, 1.0 / 2.4) - 0.055) * 255.0
    }
}

#[inline]
fn chromatic_adaptation(comp: f64) -> f64 {
    let af = libm::pow(libm::fabs(comp), 0.42);
    let s = if comp < 0.0 {
        -1.0
    } else if comp > 0.0 {
        1.0
    } else {
        0.0
    };
    s * 400.0 * af / (af + 27.13)
}

#[inline]
fn hue_of(linrgb: Vec3) -> f64 {
    let scaled = matrix_multiply(linrgb, &SCALED_DISCOUNT_FROM_LINRGB);
    let r_a = chromatic_adaptation(scaled.a);
    let g_a = chromatic_adaptation(scaled.b);
    let b_a = chromatic_adaptation(scaled.c);
    let a = (11.0 * r_a - 12.0 * g_a + b_a) / 11.0;
    let b = (r_a + g_a - 2.0 * b_a) / 9.0;
    libm::atan2(b, a)
}

#[inline]
fn are_in_cyclic_order(a: f64, b: f64, c: f64) -> bool {
    let delta_a_b = sanitize_radians(b - a);
    let delta_a_c = sanitize_radians(c - a);
    delta_a_b < delta_a_c
}

#[inline]
fn intercept(source: f64, mid: f64, target: f64) -> f64 {
    (mid - source) / (target - source)
}

#[inline]
fn lerp_point(source: Vec3, t: f64, target: Vec3) -> Vec3 {
    Vec3::new(
        source.a + (target.a - source.a) * t,
        source.b + (target.b - source.b) * t,
        source.c + (target.c - source.c) * t,
    )
}

#[inline]
fn get_axis(v: Vec3, axis: usize) -> f64 {
    match axis {
        0 => v.a,
        1 => v.b,
        2 => v.c,
        _ => -1.0,
    }
}

#[inline]
fn set_coordinate(source: Vec3, coordinate: f64, target: Vec3, axis: usize) -> Vec3 {
    let t = intercept(get_axis(source, axis), coordinate, get_axis(target, axis));
    lerp_point(source, t, target)
}

#[inline]
fn is_bounded(x: f64) -> bool {
    (0.0..=100.0).contains(&x)
}

fn nth_vertex(y: f64, n: usize) -> Vec3 {
    let k_r = K_Y_FROM_LINRGB[0];
    let k_g = K_Y_FROM_LINRGB[1];
    let k_b = K_Y_FROM_LINRGB[2];
    let coord_a = if n % 4 <= 1 { 0.0 } else { 100.0 };
    #[allow(clippy::manual_is_multiple_of)]
    let coord_b = if n % 2 == 0 { 0.0 } else { 100.0 };
    if n < 4 {
        let g = coord_a;
        let b = coord_b;
        let r = (y - g * k_g - b * k_b) / k_r;
        if is_bounded(r) {
            Vec3::new(r, g, b)
        } else {
            Vec3::new(-1.0, -1.0, -1.0)
        }
    } else if n < 8 {
        let b = coord_a;
        let r = coord_b;
        let g = (y - r * k_r - b * k_b) / k_g;
        if is_bounded(g) {
            Vec3::new(r, g, b)
        } else {
            Vec3::new(-1.0, -1.0, -1.0)
        }
    } else {
        let r = coord_a;
        let g = coord_b;
        let b = (y - r * k_r - g * k_g) / k_b;
        if is_bounded(b) {
            Vec3::new(r, g, b)
        } else {
            Vec3::new(-1.0, -1.0, -1.0)
        }
    }
}

fn bisect_to_segment(y: f64, target_hue: f64) -> (Vec3, Vec3) {
    let mut left = Vec3::new(-1.0, -1.0, -1.0);
    let mut right = left;
    let mut left_hue = 0.0;
    let mut right_hue = 0.0;
    let mut initialized = false;
    let mut uncut = true;

    for n in 0..12 {
        let mid = nth_vertex(y, n);
        if mid.a < 0.0 {
            continue;
        }
        let mid_hue = hue_of(mid);
        if !initialized {
            left = mid;
            right = mid;
            left_hue = mid_hue;
            right_hue = mid_hue;
            initialized = true;
            continue;
        }
        if uncut || are_in_cyclic_order(left_hue, mid_hue, right_hue) {
            uncut = false;
            if are_in_cyclic_order(left_hue, target_hue, mid_hue) {
                right = mid;
                right_hue = mid_hue;
            } else {
                left = mid;
                left_hue = mid_hue;
            }
        }
    }
    (left, right)
}

#[inline]
fn inverse_chromatic_adaptation(adapted: f64) -> f64 {
    let adapted_abs = libm::fabs(adapted);
    let base = libm::fmax(0.0, 27.13 * adapted_abs / (400.0 - adapted_abs));
    let s = if adapted < 0.0 {
        -1.0
    } else if adapted > 0.0 {
        1.0
    } else {
        0.0
    };
    s * libm::pow(base, 1.0 / 0.42)
}

fn find_result_by_j(hue_radians: f64, chroma: f64, y: f64) -> Option<Rgba> {
    let mut j = libm::sqrt(y) * 11.0;
    let t_inner_coeff = 1.0 / libm::pow(1.64 - libm::pow(0.29, 0.184186503), 0.73);
    let e_hue = 0.25 * (libm::cos(hue_radians + 2.0) + 3.8);
    let p1 = e_hue * (50000.0 / 13.0) * 1.0 * 1.016919255;
    let h_sin = libm::sin(hue_radians);
    let h_cos = libm::cos(hue_radians);

    for iteration in 0..5 {
        let j_normalized = j / 100.0;
        let alpha = if chroma == 0.0 || j == 0.0 {
            0.0
        } else {
            chroma / libm::sqrt(j_normalized)
        };
        let t = libm::pow(alpha * t_inner_coeff, 1.0 / 0.9);
        let ac = 29.981000900 * libm::pow(j_normalized, 1.0 / 0.689999998 / 1.909169555);
        let p2 = ac / 1.016919255;
        let gamma = 23.0 * (p2 + 0.305) * t / (23.0 * p1 + 11.0 * t * h_cos + 108.0 * t * h_sin);
        let a = gamma * h_cos;
        let b = gamma * h_sin;
        let r_a = (460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0;
        let g_a = (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0;
        let b_a = (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0;

        let r_c_scaled = inverse_chromatic_adaptation(r_a);
        let g_c_scaled = inverse_chromatic_adaptation(g_a);
        let b_c_scaled = inverse_chromatic_adaptation(b_a);

        let scaled = Vec3::new(r_c_scaled, g_c_scaled, b_c_scaled);
        let linrgb = matrix_multiply(scaled, &LINRGB_FROM_SCALED_DISCOUNT);

        if linrgb.a < 0.0 || linrgb.b < 0.0 || linrgb.c < 0.0 {
            return None;
        }

        let fnj = K_Y_FROM_LINRGB[0] * linrgb.a
            + K_Y_FROM_LINRGB[1] * linrgb.b
            + K_Y_FROM_LINRGB[2] * linrgb.c;
        if fnj <= 0.0 {
            return None;
        }

        if iteration == 4 || libm::fabs(fnj - y) < 0.002 {
            if linrgb.a > 100.01 || linrgb.b > 100.01 || linrgb.c > 100.01 {
                return None;
            }
            return Some(Rgba::new(
                linear_to_srgb(linrgb.a / 100.0),
                linear_to_srgb(linrgb.b / 100.0),
                linear_to_srgb(linrgb.c / 100.0),
                255,
            ));
        }

        j -= (fnj - y) * j / (2.0 * fnj);
    }

    None
}

const CRITICAL_PLANES: [f64; 255] = [
    0.015176349177441876,
    0.045529047532325624,
    0.07588174588720938,
    0.10623444424209313,
    0.13658714259697685,
    0.16693984095186062,
    0.19729253930674434,
    0.2276452376616281,
    0.2579979360165119,
    0.28835063437139563,
    0.3188300904430532,
    0.350925934958123,
    0.3848314933096426,
    0.42057480301049466,
    0.458183274052838,
    0.4976837250274023,
    0.5391024159806381,
    0.5824650784040898,
    0.6277969426914107,
    0.6751227633498623,
    0.7244668422128921,
    0.775853049866786,
    0.829304845476233,
    0.8848452951698498,
    0.942497089126609,
    1.0022825574869039,
    1.0642236851973577,
    1.1283421258858297,
    1.1946592148522128,
    1.2631959812511864,
    1.3339731595349034,
    1.407011200216447,
    1.4823302800086415,
    1.5599503113873272,
    1.6398909516233677,
    1.7221716113234105,
    1.8068114625156377,
    1.8938294463134073,
    1.9832442801866852,
    2.075074464868551,
    2.1693382909216234,
    2.2660538449872063,
    2.36523901573795,
    2.4669114995532007,
    2.5710888059345764,
    2.6777882626779785,
    2.7870270208169257,
    2.898822059350997,
    3.0131901897720907,
    3.1301480604002863,
    3.2497121605402226,
    3.3718988244681087,
    3.4967242352587946,
    3.624204428461639,
    3.754355295633311,
    3.887192587735158,
    4.022731918402185,
    4.160988767090289,
    4.301978482107941,
    4.445716283538092,
    4.592217266055746,
    4.741496401646282,
    4.893568542229298,
    5.048448422192488,
    5.20615066083972,
    5.3666897647573375,
    5.5300801301023865,
    5.696336044816294,
    5.865471690767354,
    6.037501145825082,
    6.212438385869475,
    6.390297286737924,
    6.571091626112461,
    6.7548350853498045,
    6.941541251256611,
    7.131223617812143,
    7.323895587840543,
    7.5195704746346665,
    7.7182615035334345,
    7.919981813454504,
    8.124744458384042,
    8.332562408825165,
    8.543448553206703,
    8.757415699253682,
    8.974476575321063,
    9.194643831691977,
    9.417930041841839,
    9.644347703669503,
    9.873909240696694,
    10.106627003236781,
    10.342513269534024,
    10.58158024687427,
    10.8238400726681,
    11.069304815507364,
    11.317986476196008,
    11.569896988756009,
    11.825048221409341,
    12.083451977536606,
    12.345119996613247,
    12.610063955123938,
    12.878295467455942,
    13.149826086772048,
    13.42466730586372,
    13.702830557985108,
    13.984327217668513,
    14.269168601521828,
    14.55736596900856,
    14.848930523210871,
    15.143873411576273,
    15.44220572664832,
    15.743938506781891,
    16.04908273684337,
    16.35764934889634,
    16.66964922287304,
    16.985093187232053,
    17.30399201960269,
    17.62635644741625,
    17.95219714852476,
    18.281524751807332,
    18.614349837764564,
    18.95068293910138,
    19.290534541298456,
    19.633915083172692,
    19.98083495742689,
    20.331304511189067,
    20.685334046541502,
    21.042933821039977,
    21.404114048223256,
    21.76888489811322,
    22.137256497705877,
    22.50923893145328,
    22.884842241736916,
    23.264076429332462,
    23.6469514538663,
    24.033477234264016,
    24.42366364919083,
    24.817520537484558,
    25.21505769858089,
    25.61628489293138,
    26.021211842414342,
    26.429848230738664,
    26.842203703840827,
    27.258287870275353,
    27.678110301598522,
    28.10168053274597,
    28.529008062403893,
    28.96010235337422,
    29.39497283293396,
    29.83362889318845,
    30.276079891419332,
    30.722335150426627,
    31.172403958865512,
    31.62629557157785,
    32.08401920991837,
    32.54558406207592,
    33.010999283389665,
    33.4802739966603,
    33.953417292456834,
    34.430438229418264,
    34.911345834551085,
    35.39614910352207,
    35.88485700094671,
    36.37747846067349,
    36.87402238606382,
    37.37449765026789,
    37.87891309649659,
    38.38727753828926,
    38.89959975977785,
    39.41588851594697,
    39.93615253289054,
    40.460400508064545,
    40.98864111053629,
    41.520882981230194,
    42.05713473317016,
    42.597404951718396,
    43.141702194811224,
    43.6900349931913,
    44.24241185063697,
    44.798841244188324,
    45.35933162437017,
    45.92389141541209,
    46.49252901546552,
    47.065252796817916,
    47.64207110610409,
    48.22299226451468,
    48.808024568002054,
    49.3971762874833,
    49.9904556690408,
    50.587870934119984,
    51.189430279724725,
    51.79514187861014,
    52.40501387947288,
    53.0190544071392,
    53.637271562750364,
    54.259673423945976,
    54.88626804504493,
    55.517063457223934,
    56.15206766869424,
    56.79128866487574,
    57.43473440856916,
    58.08241284012621,
    58.734331877617365,
    59.39049941699807,
    60.05092333227251,
    60.715611475655585,
    61.38457167773311,
    62.057811747619894,
    62.7353394731159,
    63.417162620860914,
    64.10328893648692,
    64.79372614476921,
    65.48848194977529,
    66.18756403501224,
    66.89098006357258,
    67.59873767827808,
    68.31084450182222,
    69.02730813691093,
    69.74813616640164,
    70.47333615344107,
    71.20291564160104,
    71.93688215501312,
    72.67524319850172,
    73.41800625771542,
    74.16517879925733,
    74.9167682708136,
    75.67278210128072,
    76.43322770089146,
    77.1981124613393,
    77.96744375590167,
    78.74122893956174,
    79.51947534912904,
    80.30219030335869,
    81.08938110306934,
    81.88105503125999,
    82.67721935322541,
    83.4778813166706,
    84.28304815182372,
    85.09272707154808,
    85.90692527145302,
    86.72564993000343,
    87.54890820862819,
    88.3767072518277,
    89.2090541872801,
    90.04595612594655,
    90.88742016217518,
    91.73345337380438,
    92.58406282226491,
    93.43925555268066,
    94.29903859396902,
    95.16341895893969,
    96.03240364439274,
    96.9059996312159,
    97.78421388448044,
    98.6670533535366,
    99.55452497210776,
];

fn bisect_to_limit(y: f64, target_hue: f64) -> Vec3 {
    let (mut left, mut right) = bisect_to_segment(y, target_hue);
    let mut left_hue = hue_of(left);

    for axis in 0..3 {
        if get_axis(left, axis) != get_axis(right, axis) {
            let (mut l_plane, mut r_plane) = if get_axis(left, axis) < get_axis(right, axis) {
                (
                    libm::floor(true_delinearized(get_axis(left, axis)) - 0.5) as i32,
                    libm::ceil(true_delinearized(get_axis(right, axis)) - 0.5) as i32,
                )
            } else {
                (
                    libm::ceil(true_delinearized(get_axis(left, axis)) - 0.5) as i32,
                    libm::floor(true_delinearized(get_axis(right, axis)) - 0.5) as i32,
                )
            };

            for _ in 0..8 {
                if (r_plane - l_plane).abs() <= 1 {
                    break;
                } else {
                    let m_plane = libm::floor((l_plane + r_plane) as f64 / 2.0) as i32;
                    let idx = m_plane.clamp(0, 254) as usize;
                    let mid_plane_coord = CRITICAL_PLANES[idx];
                    let mid = set_coordinate(left, mid_plane_coord, right, axis);
                    let mid_hue = hue_of(mid);
                    if are_in_cyclic_order(left_hue, target_hue, mid_hue) {
                        right = mid;
                        r_plane = m_plane;
                    } else {
                        left = mid;
                        left_hue = mid_hue;
                        l_plane = m_plane;
                    }
                }
            }
        }
    }

    Vec3::new(
        (left.a + right.a) / 2.0,
        (left.b + right.b) / 2.0,
        (left.c + right.c) / 2.0,
    )
}

#[inline]
pub fn y_from_lstar(lstar: f64) -> f64 {
    lab_inv_f((lstar + 16.0) / 116.0) * 100.0
}

#[inline]
pub fn lstar_from_y(y: f64) -> f64 {
    lab_f(y / 100.0) * 116.0 - 16.0
}

pub fn solve_hct_to_rgba(hue_degrees: f64, chroma: f64, lstar: f64) -> Rgba {
    if chroma < 0.0001 || lstar < 0.0001 {
        let val = linear_to_srgb(y_from_lstar(lstar) / 100.0);
        return Rgba::new(val, val, val, 255);
    }
    if lstar > 99.9999 {
        return Rgba::new(255, 255, 255, 255);
    }

    let hue_deg = sanitize_degrees(hue_degrees);
    let hue_rad = (hue_deg / 180.0) * PI;
    let y = y_from_lstar(lstar);

    if let Some(rgb) = find_result_by_j(hue_rad, chroma, y) {
        return rgb;
    }

    let linrgb = bisect_to_limit(y, hue_rad);
    Rgba::new(
        linear_to_srgb(linrgb.a / 100.0),
        linear_to_srgb(linrgb.b / 100.0),
        linear_to_srgb(linrgb.c / 100.0),
        255,
    )
}
