//! Scoring and ranking of quantized color clusters.
//!
//! Evaluates candidate colors based on:
//! - Achromatic check
//! - 360° hue-neighborhood excited population weighting
//! - Formula: `population^0.38 * chroma * mid-tone lightness penalty/bonus`
//! - Skin-tone penalty (avoiding flesh tones as vibrant primary accents)

use crate::palette::color::{Hct, Rgba};
use crate::palette::quantize::QuantizedCluster;
use alloc::vec::Vec;

/// Checks if a color is achromatic (neutral / grayscale).
#[inline]
pub fn is_achromatic(hct: &Hct) -> bool {
    hct.chroma < 4.0 || hct.tone < 10.0 || hct.tone > 98.0
}

/// Checks if a color is within the human skin tone / flesh tone region in HCT space.
///
/// Typical human skin tones:
/// - CAM16 Hue: ~20° to 70° (orange to warm yellow-red)
/// - Chroma: ~10 to 45
/// - Tone: ~25 to 85
#[inline]
pub fn is_skin_tone(hct: &Hct) -> bool {
    (20.0..=70.0).contains(&hct.hue)
        && (10.0..=45.0).contains(&hct.chroma)
        && (25.0..=85.0).contains(&hct.tone)
}

/// Scored candidate color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoredColor {
    pub color: Rgba,
    pub hct: Hct,
    pub score: f64,
    pub population: usize,
}

/// Score and rank quantized clusters.
///
/// Uses the formula:
/// `score = (excited_population ^ 0.38) * chroma * mid_tone_factor * skin_penalty`
///
/// Where:
/// - `excited_population`: sum of populations across a 30° window (hue - 14 to hue + 15)
/// - `chroma`: CAM16 chroma (capped to prevent excessively garish colors from drowning others)
/// - `mid_tone_factor`: bonus for pleasant mid-tones around tone 50, penalty for extreme dark/light
/// - `skin_penalty`: 0.1x penalty for colors falling within skin tone bounds
pub fn score_clusters(clusters: &[QuantizedCluster]) -> Vec<ScoredColor> {
    if clusters.is_empty() {
        return Vec::new();
    }

    // Step 1: Compute per-degree hue population across 360 degrees
    let mut hue_population = [0.0f64; 360];
    let mut total_population = 0.0f64;

    let mut cluster_hcts = Vec::with_capacity(clusters.len());

    for cluster in clusters {
        let hct = Hct::from_rgba(cluster.color);
        let pop = cluster.population as f64;
        let deg = (libm::round(hct.hue) as usize) % 360;
        hue_population[deg] += pop;
        total_population += pop;
        cluster_hcts.push((cluster, hct));
    }

    if total_population <= 0.0 {
        return Vec::new();
    }

    // Step 2: Compute 30-degree excited proportion for each hue degree
    let mut excited_proportions = [0.0f64; 360];
    for (hue, &pop) in hue_population.iter().enumerate() {
        let proportion = pop / total_population;
        if proportion <= 0.0 {
            continue;
        }
        for offset in -14..=15 {
            let neighbor = ((hue as isize + offset).rem_euclid(360)) as usize;
            excited_proportions[neighbor] += proportion;
        }
    }

    // Step 3: Score each cluster
    let mut scored = Vec::with_capacity(cluster_hcts.len());

    for (cluster, hct) in cluster_hcts {
        if is_achromatic(&hct) {
            continue;
        }

        let deg = (libm::round(hct.hue) as usize) % 360;
        let excited_prop = excited_proportions[deg];

        // population^0.38
        let pop_factor = libm::pow(excited_prop.max(0.0001), 0.38);

        // chroma weight (bonus up to ~65)
        let chroma_factor = hct.chroma.min(70.0);

        // mid-tone lightness factor: peaks near tone 50, drops near 0 or 100
        // Gaussian-like curve around tone 50: 1.0 - ((tone - 50)/50)^2
        let tone_diff = (hct.tone - 50.0) / 50.0;
        let mid_tone_factor = (1.0 - (tone_diff * tone_diff)).max(0.1);

        // Skin tone penalty: avoid turning an actor/person face into dominant theme primary
        let skin_penalty = if is_skin_tone(&hct) { 0.15 } else { 1.0 };

        let score = pop_factor * chroma_factor * mid_tone_factor * skin_penalty;

        scored.push(ScoredColor {
            color: cluster.color,
            hct,
            score,
            population: cluster.population,
        });
    }

    // Sort descending by score
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    scored
}
