//! Retention and backdrop policy vocabulary.
//!
//! Shared types for cache eviction ladders, recording retention budgets,
//! and visual fallback / poster generation policies.

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

use crate::caps::DegradationTier;

/// Strategy used to evict items when a size budget is exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum EvictOrder {
    /// Least recently used item is evicted first.
    Lru,
    /// Oldest item (by recording or generation timestamp) is evicted first.
    OldestFirst,
    /// Lowest degradation tier (e.g., Tier4PosterFallback, then Tier3, etc.) evicted first.
    LowestTierFirst,
}

/// Retention policies for stream chunks, recordings, and cached artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum RetentionPolicy {
    /// Keep the most recent `n` items.
    KeepLast { n: usize },
    /// Keep items whose age is within `ttl_secs`.
    TimeToLive { ttl_secs: u64 },
    /// Keep items within a total size budget in bytes, evicting by `evict_order`.
    SizeBudget {
        max_bytes: u64,
        evict_order: EvictOrder,
    },
    /// Tier-specific retention policies evaluated in degradation ladder sequence.
    Tiered(Vec<(DegradationTier, RetentionPolicy)>),
}

/// Keyframe selection heuristic for poster/backdrop extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum BackdropSelect {
    /// Pick the first keyframe found in the stream or sequence.
    FirstKeyframe,
    /// Pick the keyframe with highest visual sharpness (e.g. luma variance).
    Sharpest,
    /// Pick the keyframe closest to the temporal midpoint.
    Midpoint,
}

/// Policy governing poster/backdrop generation from video streams.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct BackdropPolicy {
    /// Tiers this policy produces or targets (e.g., full-res, preview, poster).
    pub tiers: Vec<DegradationTier>,
    /// Maximum edge dimension in pixels for generated backdrops.
    pub max_edge_px: u32,
    /// MIME preference order (e.g., `["image/jpeg", "image/webp", "image/avif"]`).
    pub mime_budget: Vec<String>,
    /// Frame selection heuristic.
    pub select_at: BackdropSelect,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_policy_construction() {
        let p = RetentionPolicy::SizeBudget {
            max_bytes: 1024 * 1024,
            evict_order: EvictOrder::Lru,
        };
        assert!(matches!(p, RetentionPolicy::SizeBudget { .. }));

        let tiered = RetentionPolicy::Tiered(alloc::vec![
            (
                DegradationTier::Tier1HwSurface,
                RetentionPolicy::KeepLast { n: 5 }
            ),
            (
                DegradationTier::Tier4PosterFallback,
                RetentionPolicy::TimeToLive { ttl_secs: 3600 }
            ),
        ]);
        if let RetentionPolicy::Tiered(tiers) = tiered {
            assert_eq!(tiers.len(), 2);
        }
    }

    #[test]
    fn backdrop_policy_construction() {
        let bp = BackdropPolicy {
            tiers: alloc::vec![DegradationTier::Tier4PosterFallback],
            max_edge_px: 1280,
            mime_budget: alloc::vec![String::from("image/jpeg"), String::from("image/webp")],
            select_at: BackdropSelect::Midpoint,
        };
        assert_eq!(bp.max_edge_px, 1280);
        assert_eq!(bp.select_at, BackdropSelect::Midpoint);
    }
}
