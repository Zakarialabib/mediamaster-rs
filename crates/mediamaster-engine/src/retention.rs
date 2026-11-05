//! Retention policy engine.
//!
//! Evaluates retention policies against catalogs of retained media items
//! without side effects or I/O, yielding pure `EvictionPlan` structures.

use mediamaster_core::caps::DegradationTier;
use mediamaster_core::events::{EventPublisher, HardwareEvent};
use mediamaster_core::policy::{EvictOrder, RetentionPolicy};
use std::collections::{BTreeMap, HashMap};

/// Item metadata required by the retention policy engine.
pub trait RetainedItem {
    /// Unique identifier of the item.
    fn id(&self) -> &str;
    /// Identifier of the stream or logical recording channel this item belongs to.
    fn stream_id(&self) -> &str;
    /// Degradation tier of this item.
    fn tier(&self) -> DegradationTier;
    /// Item creation / recording timestamp (seconds or milliseconds, matching `now` / `ttl_secs`).
    fn timestamp(&self) -> u64;
    /// Item last access timestamp.
    fn last_accessed(&self) -> u64;
    /// Size of the item artifact in bytes.
    fn size_bytes(&self) -> u64;
}

/// Eviction plan computed by the retention engine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvictionPlan {
    /// Item IDs selected for eviction.
    pub evict_ids: Vec<String>,
    /// Total bytes that will be freed by executing this plan.
    pub freed_bytes: u64,
}

impl EvictionPlan {
    /// Creates an empty eviction plan.
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits a `HardwareEvent::EvictionPlanned` event on the provided event publisher.
    pub fn emit_event(&self, publisher: &dyn EventPublisher) {
        publisher.publish(HardwareEvent::EvictionPlanned {
            count: self.evict_ids.len(),
            freed_bytes: self.freed_bytes,
        });
    }
}

/// Engine that computes pure eviction plans against retention policies.
pub struct RetentionEngine;

impl RetentionEngine {
    /// Evaluates `policy` over `items` at timestamp `now`.
    ///
    /// # Invariant
    /// Guarantees that at least one item is kept per unique `stream_id`.
    /// The protected item per stream is chosen as the newest item (highest timestamp,
    /// break ties with last_accessed or id).
    pub fn plan_eviction<'a, I, T>(items: I, policy: &RetentionPolicy, now: u64) -> EvictionPlan
    where
        I: IntoIterator<Item = &'a T>,
        T: RetainedItem + 'a,
    {
        let all_items: Vec<&'a T> = items.into_iter().collect();
        if all_items.is_empty() {
            return EvictionPlan::new();
        }

        // Identify the protected item per stream (must keep >= 1 item per stream)
        let mut stream_protected: HashMap<&str, &str> = HashMap::new();
        for item in &all_items {
            let stream = item.stream_id();
            match stream_protected.get(stream) {
                None => {
                    stream_protected.insert(stream, item.id());
                }
                Some(&current_best_id) => {
                    let current_best = all_items
                        .iter()
                        .find(|i| i.id() == current_best_id)
                        .unwrap();
                    // Prefer higher timestamp; then higher last_accessed; then lexicographically higher id
                    if (item.timestamp(), item.last_accessed(), item.id())
                        > (
                            current_best.timestamp(),
                            current_best.last_accessed(),
                            current_best.id(),
                        )
                    {
                        stream_protected.insert(stream, item.id());
                    }
                }
            }
        }

        let evict_ids = Self::eval_policy(&all_items, policy, now, &stream_protected);
        let mut freed_bytes = 0u64;
        let id_set: std::collections::HashSet<&str> =
            evict_ids.iter().map(|s| s.as_str()).collect();
        for item in &all_items {
            if id_set.contains(item.id()) {
                freed_bytes = freed_bytes.saturating_add(item.size_bytes());
            }
        }

        EvictionPlan {
            evict_ids,
            freed_bytes,
        }
    }

    fn eval_policy<'a, T: RetainedItem + 'a>(
        items: &[&'a T],
        policy: &RetentionPolicy,
        now: u64,
        stream_protected: &HashMap<&str, &str>,
    ) -> Vec<String> {
        match policy {
            RetentionPolicy::KeepLast { n } => {
                // Keep the most recent `n` items per stream (or overall).
                // In streaming/catalog applications, KeepLast operates per stream so that
                // one active stream doesn't starve other streams while keeping >= 1 item per stream.
                let mut per_stream: HashMap<&str, Vec<&'a T>> = HashMap::new();
                for &item in items {
                    per_stream.entry(item.stream_id()).or_default().push(item);
                }

                let mut to_evict = Vec::new();
                for (stream, mut stream_items) in per_stream {
                    // Sort newest first
                    stream_items.sort_by(|a, b| {
                        (b.timestamp(), b.last_accessed(), b.id()).cmp(&(
                            a.timestamp(),
                            a.last_accessed(),
                            a.id(),
                        ))
                    });

                    let protected_id = stream_protected.get(stream).copied();
                    let effective_n = (*n).max(1); // Always keep at least 1 item per stream
                    for (idx, item) in stream_items.iter().enumerate() {
                        if idx >= effective_n && Some(item.id()) != protected_id {
                            to_evict.push(item.id().to_string());
                        }
                    }
                }
                to_evict
            }
            RetentionPolicy::TimeToLive { ttl_secs } => {
                let mut to_evict = Vec::new();
                let threshold = now.saturating_sub(*ttl_secs);
                for &item in items {
                    let protected_id = stream_protected.get(item.stream_id()).copied();
                    if item.timestamp() < threshold {
                        // Evict if not protected
                        if Some(item.id()) != protected_id {
                            to_evict.push(item.id().to_string());
                        }
                    }
                }
                to_evict
            }
            RetentionPolicy::SizeBudget {
                max_bytes,
                evict_order,
            } => {
                let total_bytes: u64 = items.iter().map(|i| i.size_bytes()).sum();
                if total_bytes <= *max_bytes {
                    return Vec::new();
                }

                // Partition items into candidate (evictable) vs protected
                let mut candidates: Vec<&'a T> = items
                    .iter()
                    .copied()
                    .filter(|item| {
                        let protected_id = stream_protected.get(item.stream_id()).copied();
                        Some(item.id()) != protected_id
                    })
                    .collect();

                // Sort candidates according to evict_order
                match evict_order {
                    EvictOrder::Lru => {
                        // Evict lowest last_accessed first; tie-break by timestamp then id
                        candidates.sort_by(|a, b| {
                            (a.last_accessed(), a.timestamp(), a.id()).cmp(&(
                                b.last_accessed(),
                                b.timestamp(),
                                b.id(),
                            ))
                        });
                    }
                    EvictOrder::OldestFirst => {
                        // Evict lowest timestamp first
                        candidates.sort_by(|a, b| {
                            (a.timestamp(), a.last_accessed(), a.id()).cmp(&(
                                b.timestamp(),
                                b.last_accessed(),
                                b.id(),
                            ))
                        });
                    }
                    EvictOrder::LowestTierFirst => {
                        // DegradationTier: Tier1HwSurface < Tier2HwByteBuffer < Tier3SharedTextureMux < Tier4PosterFallback.
                        // "LowestTierFirst": Lowest tier in quality / fallback hierarchy is Tier4PosterFallback,
                        // or by tier value descending. Let's make sure Tier4 is evicted before Tier3, Tier2, Tier1.
                        // Order by tier descending (Tier4 first), then oldest first.
                        candidates.sort_by(|a, b| {
                            (std::cmp::Reverse(a.tier()), a.timestamp(), a.id()).cmp(&(
                                std::cmp::Reverse(b.tier()),
                                b.timestamp(),
                                b.id(),
                            ))
                        });
                    }
                }

                let mut current_total = total_bytes;
                let mut to_evict = Vec::new();
                for candidate in candidates {
                    if current_total <= *max_bytes {
                        break;
                    }
                    to_evict.push(candidate.id().to_string());
                    current_total = current_total.saturating_sub(candidate.size_bytes());
                }
                to_evict
            }
            RetentionPolicy::Tiered(tiers) => {
                // Tier-specific retention policies
                let mut tier_map: BTreeMap<DegradationTier, &RetentionPolicy> = BTreeMap::new();
                for (tier, p) in tiers {
                    tier_map.insert(*tier, p);
                }

                let mut to_evict = Vec::new();
                for (tier, sub_policy) in tier_map {
                    let tier_items: Vec<&'a T> =
                        items.iter().copied().filter(|i| i.tier() == tier).collect();
                    if !tier_items.is_empty() {
                        let evictions =
                            Self::eval_policy(&tier_items, sub_policy, now, stream_protected);
                        to_evict.extend(evictions);
                    }
                }
                to_evict
            }
        }
    }
}
