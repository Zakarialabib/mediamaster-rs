use crate::palette::color::{Hct, Oklab, Rgba};
use crate::palette::scheme::PaletteEngine;
use crate::palette::score::is_skin_tone;
use alloc::vec;
use alloc::vec::Vec;

// Helper to generate a solid or bordered artwork
fn make_artwork(
    w: usize,
    h: usize,
    border_color: Option<Rgba>,
    border_size: usize,
    dominant: Rgba,
    accents: &[(Rgba, usize)],
) -> Vec<Rgba> {
    let mut pixels = vec![dominant; w * h];

    // Draw border if requested
    if let Some(bc) = border_color {
        for y in 0..h {
            for x in 0..w {
                if x < border_size
                    || x >= w - border_size
                    || y < border_size
                    || y >= h - border_size
                {
                    pixels[y * w + x] = bc;
                }
            }
        }
    }

    // Draw accent patches
    let mut offset = border_size + 5;
    for &(acc_color, count) in accents {
        for _ in 0..count {
            if offset < pixels.len() {
                pixels[offset] = acc_color;
                offset += 7;
            }
        }
    }

    pixels
}

#[test]
fn test_wcag_contrast_levels() {
    // High contrast pair (white on black)
    let white = Rgba::new(255, 255, 255, 255);
    let black = Rgba::new(0, 0, 0, 255);
    assert!(white.contrast_ratio(&black) >= 20.0);
    assert!(white.is_wcag_aa(&black));
    assert!(white.is_wcag_aaa(&black));

    // Gray on white low contrast
    let light_gray = Rgba::new(200, 200, 200, 255);
    assert!(!light_gray.is_wcag_aa(&white));
}

#[test]
fn test_ten_reference_artworks_delta_e_and_wcag() {
    // 10 distinct reference signage artwork configurations
    let references: [(Rgba, &str); 10] = [
        (Rgba::rgb(0x19, 0x76, 0xd2), "Material Blue"),
        (Rgba::rgb(0x38, 0x8e, 0x3c), "Forest Green"),
        (Rgba::rgb(0xd3, 0x2f, 0x2f), "Crimson Red"),
        (Rgba::rgb(0x7b, 0x1f, 0xa2), "Deep Purple"),
        (Rgba::rgb(0xf5, 0x7c, 0x00), "Vibrant Amber"),
        (Rgba::rgb(0x00, 0x97, 0xa7), "Cyan Teal"),
        (Rgba::rgb(0xc2, 0x18, 0x5b), "Rose Pink"),
        (Rgba::rgb(0x51, 0x2d, 0xa8), "Indigo Night"),
        (Rgba::rgb(0x68, 0x9f, 0x38), "Lime Olive"),
        (Rgba::rgb(0x02, 0x88, 0xd1), "Sky Ocean"),
    ];

    let mut engine = PaletteEngine::new();

    for (target_color, name) in references {
        let width = 160;
        let height = 120;
        // Add a uniform black border (<= 15% on each edge) to verify border trim
        let artwork = make_artwork(
            width,
            height,
            Some(Rgba::rgb(0, 0, 0)),
            12, // 12/120 = 10% <= 22%
            target_color,
            &[(Rgba::rgb(240, 240, 240), 20)],
        );

        let palette = engine.extract_palette(&artwork, width, height);

        // Compare extracted OKLab centroid against target OKLab
        let target_oklab = Oklab::from_rgba(target_color);
        let delta_e = palette.primary_oklab.delta_e(&target_oklab);

        // Exit criterion: 10 reference artworks within ±1 ΔE
        assert!(
            delta_e <= 1.0,
            "Artwork '{}' exceeded delta_e: {} > 1.0 (got {:?}, target {:?})",
            name,
            delta_e,
            palette.primary_color,
            target_color
        );

        // Exit criterion: pass WCAG AA
        // Check light scheme primary against on_primary and surface against on_surface
        let light = &palette.scheme.light;
        assert!(
            light.primary.is_wcag_aa(&light.on_primary),
            "Artwork '{}' light primary/on_primary failed WCAG AA: ratio {}",
            name,
            light.primary.contrast_ratio(&light.on_primary)
        );
        assert!(
            light.surface.is_wcag_aa(&light.on_surface),
            "Artwork '{}' light surface/on_surface failed WCAG AA: ratio {}",
            name,
            light.surface.contrast_ratio(&light.on_surface)
        );

        // Check dark scheme primary against on_primary and surface against on_surface
        let dark = &palette.scheme.dark;
        assert!(
            dark.primary.is_wcag_aa(&dark.on_primary),
            "Artwork '{}' dark primary/on_primary failed WCAG AA: ratio {}",
            name,
            dark.primary.contrast_ratio(&dark.on_primary)
        );
        assert!(
            dark.surface.is_wcag_aa(&dark.on_surface),
            "Artwork '{}' dark surface/on_surface failed WCAG AA: ratio {}",
            name,
            dark.surface.contrast_ratio(&dark.on_surface)
        );
    }
}

#[test]
fn test_skin_tone_image_does_not_produce_skin_tone_primary() {
    // Create an image where the large background is a human skin tone (e.g. portrait photo background / face)
    // Caucasian / warm skin tone: ~#e8beac -> RGB(232, 190, 172)
    let skin_color = Rgba::rgb(232, 190, 172);
    let skin_hct = Hct::from_rgba(skin_color);
    assert!(
        is_skin_tone(&skin_hct),
        "skin_color should be recognized as skin tone: {:?}",
        skin_hct
    );

    // Vibrant accent in the image (e.g. a teal shirt / jewel accessory)
    let accent_color = Rgba::rgb(0x00, 0x89, 0x7b); // teal

    let width = 140;
    let height = 140;

    // 75% skin tone (top 3/4 of image), 25% vibrant accent (bottom 1/4 of image)
    let mut pixels = vec![skin_color; width * height];
    for y in ((height * 3) / 4)..height {
        for x in 0..width {
            pixels[y * width + x] = accent_color;
        }
    }

    let mut engine = PaletteEngine::new();
    let palette = engine.extract_palette(&pixels, width, height);

    // Exit criterion: skin-tone image doesn't produce skin-tone primary
    let primary_hct = palette.primary_hct;
    assert!(
        !is_skin_tone(&primary_hct),
        "Primary color produced was skin tone: {:?} (RGB: {:?})",
        primary_hct,
        palette.primary_color
    );
    assert_eq!(palette.primary_color, accent_color);
}

#[test]
fn test_lru_cache_behavior() {
    let mut engine = PaletteEngine::new();
    let color1 = Rgba::rgb(10, 100, 200);
    let color2 = Rgba::rgb(200, 100, 10);

    let img1 = vec![color1; 64 * 64];
    let img2 = vec![color2; 64 * 64];

    let p1 = engine.extract_palette(&img1, 64, 64);
    let p2 = engine.extract_palette(&img2, 64, 64);
    assert_ne!(p1.primary_color, p2.primary_color);

    // Fetch from cache
    let p1_cached = engine.extract_palette(&img1, 64, 64);
    assert_eq!(p1.primary_color, p1_cached.primary_color);
}
