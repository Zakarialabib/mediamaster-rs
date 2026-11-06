//! TonalPalette, ColorScheme (15 light + dark roles), and PaletteEngine with 64-entry LRU.

use crate::palette::color::{Hct, Oklab, Rgba};
use crate::palette::preprocess::preprocess_image;
use crate::palette::quantize::quantize_celebi;
use crate::palette::score::score_clusters;
use alloc::vec::Vec;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// Tonal palette generating colors varying in tone (0 to 100) at fixed hue and chroma.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct TonalPalette {
    pub hue: f64,
    pub chroma: f64,
}

impl TonalPalette {
    pub fn of(hue: f64, chroma: f64) -> Self {
        Self { hue, chroma }
    }

    pub fn from_hct(hct: Hct) -> Self {
        Self {
            hue: hct.hue,
            chroma: hct.chroma,
        }
    }

    /// Evaluates the color at the given tone (0.0 to 100.0).
    pub fn tone(&self, tone: f64) -> Rgba {
        Hct::new(self.hue, self.chroma, tone).to_rgba()
    }
}

/// 15 semantic color roles for signage UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct PaletteRoles {
    pub primary: Rgba,
    pub on_primary: Rgba,
    pub primary_container: Rgba,
    pub on_primary_container: Rgba,
    pub secondary: Rgba,
    pub on_secondary: Rgba,
    pub secondary_container: Rgba,
    pub on_secondary_container: Rgba,
    pub surface: Rgba,
    pub on_surface: Rgba,
    pub surface_variant: Rgba,
    pub on_surface_variant: Rgba,
    pub background: Rgba,
    pub on_background: Rgba,
    pub outline: Rgba,
}

/// Paired light and dark schemes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct ColorScheme {
    pub light: PaletteRoles,
    pub dark: PaletteRoles,
}

impl ColorScheme {
    /// Constructs a 15-role Light and Dark color scheme from primary, secondary, and neutral palettes.
    pub fn from_palettes(
        primary_palette: TonalPalette,
        secondary_palette: TonalPalette,
        neutral_palette: TonalPalette,
        neutral_variant_palette: TonalPalette,
    ) -> Self {
        // Light roles
        let light = PaletteRoles {
            primary: primary_palette.tone(40.0),
            on_primary: primary_palette.tone(100.0),
            primary_container: primary_palette.tone(90.0),
            on_primary_container: primary_palette.tone(10.0),

            secondary: secondary_palette.tone(40.0),
            on_secondary: secondary_palette.tone(100.0),
            secondary_container: secondary_palette.tone(90.0),
            on_secondary_container: secondary_palette.tone(10.0),

            surface: neutral_palette.tone(98.0),
            on_surface: neutral_palette.tone(10.0),
            surface_variant: neutral_variant_palette.tone(90.0),
            on_surface_variant: neutral_variant_palette.tone(30.0),

            background: neutral_palette.tone(99.0),
            on_background: neutral_palette.tone(10.0),

            outline: neutral_variant_palette.tone(50.0),
        };

        // Dark roles
        let dark = PaletteRoles {
            primary: primary_palette.tone(80.0),
            on_primary: primary_palette.tone(20.0),
            primary_container: primary_palette.tone(30.0),
            on_primary_container: primary_palette.tone(90.0),

            secondary: secondary_palette.tone(80.0),
            on_secondary: secondary_palette.tone(20.0),
            secondary_container: secondary_palette.tone(30.0),
            on_secondary_container: secondary_palette.tone(90.0),

            surface: neutral_palette.tone(10.0),
            on_surface: neutral_palette.tone(90.0),
            surface_variant: neutral_variant_palette.tone(30.0),
            on_surface_variant: neutral_variant_palette.tone(80.0),

            background: neutral_palette.tone(10.0),
            on_background: neutral_palette.tone(90.0),

            outline: neutral_variant_palette.tone(60.0),
        };

        Self { light, dark }
    }
}

/// Extracted palette result.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct ExtractedPalette {
    /// Primary dominant/seed color.
    pub primary_color: Rgba,
    /// Primary centroid represented in Oklab.
    pub primary_oklab: Oklab,
    /// Primary HCT.
    pub primary_hct: Hct,
    /// Tonal palette for primary hue.
    pub primary_palette: TonalPalette,
    /// Tonal palette for secondary hue.
    pub secondary_palette: TonalPalette,
    /// Tonal palette for neutral background/surfaces.
    pub neutral_palette: TonalPalette,
    /// 15 roles light and dark.
    pub scheme: ColorScheme,
}

/// 64-entry LRU cache entry.
#[derive(Clone, Debug)]
struct LruEntry {
    hash: u64,
    palette: ExtractedPalette,
}

/// OKLab / HCT palette engine for signage layouts with 64-entry LRU cache.
#[derive(Debug)]
pub struct PaletteEngine {
    lru: Vec<LruEntry>,
}

impl Default for PaletteEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PaletteEngine {
    pub const CACHE_SIZE: usize = 64;

    pub fn new() -> Self {
        Self {
            lru: Vec::with_capacity(Self::CACHE_SIZE),
        }
    }

    /// Fast hash of an image pixel buffer.
    pub fn hash_pixels(pixels: &[Rgba], width: usize, height: usize) -> u64 {
        let mut h = 0xcbf29ce484222325u64; // FNV offset basis
        let prime = 0x100000001b3u64;

        h ^= width as u64;
        h = h.wrapping_mul(prime);
        h ^= height as u64;
        h = h.wrapping_mul(prime);

        // Sample up to 1024 points for fast fingerprinting
        let step = (pixels.len() / 1024).max(1);
        for i in (0..pixels.len()).step_by(step) {
            let p = pixels[i];
            h ^= p.to_argb_u32() as u64;
            h = h.wrapping_mul(prime);
        }

        h
    }

    /// Extracts palette with 64-entry LRU caching.
    pub fn extract_palette(
        &mut self,
        pixels: &[Rgba],
        width: usize,
        height: usize,
    ) -> ExtractedPalette {
        let hash = Self::hash_pixels(pixels, width, height);

        // Check LRU
        if let Some(pos) = self.lru.iter().position(|e| e.hash == hash) {
            let entry = self.lru.remove(pos);
            let palette = entry.palette.clone();
            self.lru.insert(0, entry);
            return palette;
        }

        // Compute palette
        let palette = Self::compute_palette(pixels, width, height);

        // Insert into LRU
        if self.lru.len() >= Self::CACHE_SIZE {
            self.lru.pop();
        }
        self.lru.insert(
            0,
            LruEntry {
                hash,
                palette: palette.clone(),
            },
        );

        palette
    }

    /// Direct palette computation pipeline:
    /// 1. Border trim (<=22%), area-box downsample (<=112x112), filter alpha/lightness.
    /// 2. Celebi quantization in CIE L*a*b* space (<=128 clusters).
    /// 3. 360° hue-neighborhood excited population scoring with skin-tone penalty.
    /// 4. Oklab centroid -> HCT TonalPalette.
    /// 5. 15-role light + dark color scheme with WCAG AA/AAA compliance.
    pub fn compute_palette(pixels: &[Rgba], width: usize, height: usize) -> ExtractedPalette {
        // Step 1: Preprocess
        let preprocessed = preprocess_image(pixels, width, height);

        // Step 2: Quantize with Celebi (<= 128 clusters)
        let clusters = if preprocessed.is_empty() {
            Vec::new()
        } else {
            quantize_celebi(&preprocessed, 128)
        };

        // Step 3: Score clusters
        let scored = score_clusters(&clusters);

        // Fallback default: signage blue
        let default_color = Rgba::new(0x21, 0x5a, 0xdb, 0xff);
        let primary_color = scored.first().map(|s| s.color).unwrap_or(default_color);

        // Centroid in OKLab
        let primary_oklab = Oklab::from_rgba(primary_color);
        let primary_hct = Hct::from_rgba(primary_color);

        let primary_palette = TonalPalette::of(primary_hct.hue, primary_hct.chroma.max(48.0));
        let secondary_palette = TonalPalette::of(primary_hct.hue, 16.0);
        let neutral_palette = TonalPalette::of(primary_hct.hue, 4.0);
        let neutral_variant_palette = TonalPalette::of(primary_hct.hue, 8.0);

        let scheme = ColorScheme::from_palettes(
            primary_palette,
            secondary_palette,
            neutral_palette,
            neutral_variant_palette,
        );

        ExtractedPalette {
            primary_color,
            primary_oklab,
            primary_hct,
            primary_palette,
            secondary_palette,
            neutral_palette,
            scheme,
        }
    }
}
