//! Palette extraction and generation for signage layouts.

pub mod color;
pub mod preprocess;
pub mod quantize;
pub mod scheme;
pub mod score;

pub use color::{
    linear_to_srgb, sanitize_degrees, solve_hct_to_rgba, srgb_to_linear, Cam16, Hct, Lab, Oklab,
    Rgba,
};
pub use preprocess::preprocess_image;
pub use quantize::{quantize_celebi, quantize_wu, QuantizedCluster};
pub use scheme::{ColorScheme, ExtractedPalette, PaletteEngine, PaletteRoles, TonalPalette};
pub use score::{is_achromatic, is_skin_tone, score_clusters, ScoredColor};

#[cfg(test)]
mod tests;
