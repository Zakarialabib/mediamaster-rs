use mediamaster_core::caps::DegradationTier;
use mediamaster_core::policy::{EvictOrder, RetentionPolicy};
use mediamaster_engine::{RetainedItem, RetentionEngine};
use proptest::prelude::*;
use std::collections::HashSet;

#[derive(Debug, Clone)]
struct TestItem {
    id: String,
    stream_id: String,
    tier: DegradationTier,
    timestamp: u64,
    last_accessed: u64,
    size_bytes: u64,
}

impl RetainedItem for TestItem {
    fn id(&self) -> &str {
        &self.id
    }
    fn stream_id(&self) -> &str {
        &self.stream_id
    }
    fn tier(&self) -> DegradationTier {
        self.tier
    }
    fn timestamp(&self) -> u64 {
        self.timestamp
    }
    fn last_accessed(&self) -> u64 {
        self.last_accessed
    }
    fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

fn arb_evict_order() -> impl Strategy<Value = EvictOrder> {
    prop_oneof![
        Just(EvictOrder::Lru),
        Just(EvictOrder::OldestFirst),
        Just(EvictOrder::LowestTierFirst),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn size_budget_preserves_stream_invariant_and_budget(
        stream_count in 1..5usize,
        item_count in 1..30usize,
        max_bytes in 100u64..1_000_000u64,
        order in arb_evict_order(),
    ) {
        // Generate items
        let mut items = Vec::new();
        for i in 0..item_count {
            let s_idx = i % stream_count;
            items.push(TestItem {
                id: format!("item_{}", i),
                stream_id: format!("stream_{}", s_idx),
                tier: match i % 4 {
                    0 => DegradationTier::Tier1HwSurface,
                    1 => DegradationTier::Tier2HwByteBuffer,
                    2 => DegradationTier::Tier3SharedTextureMux,
                    _ => DegradationTier::Tier4PosterFallback,
                },
                timestamp: 1000 + i as u64,
                last_accessed: 2000 + i as u64,
                size_bytes: 500 + (i as u64 * 100),
            });
        }

        let policy = RetentionPolicy::SizeBudget {
            max_bytes,
            evict_order: order,
        };

        let plan = RetentionEngine::plan_eviction(&items, &policy, 5000);
        let evicted_set: HashSet<&str> = plan.evict_ids.iter().map(|s| s.as_str()).collect();

        // 1. Check invariant: Every stream must retain >= 1 item
        let mut retained_per_stream: HashSet<&str> = HashSet::new();
        let mut retained_total_bytes = 0u64;

        for item in &items {
            if !evicted_set.contains(item.id()) {
                retained_per_stream.insert(item.stream_id());
                retained_total_bytes += item.size_bytes();
            }
        }

        let all_streams: HashSet<&str> = items.iter().map(|i| i.stream_id()).collect();
        for stream in &all_streams {
            prop_assert!(
                retained_per_stream.contains(stream),
                "Stream {} has 0 retained items", stream
            );
        }

        // 2. Check budget: Retained total must not exceed max_bytes unless
        // the sum of the minimum single items per stream already exceeds max_bytes.
        let protected_minimum_bytes: u64 = all_streams
            .iter()
            .map(|&s| {
                // Find protected item's size
                let stream_items: Vec<&TestItem> = items.iter().filter(|i| i.stream_id() == s).collect();
                let best = stream_items.iter().max_by_key(|i| (i.timestamp(), i.last_accessed(), &i.id)).unwrap();
                best.size_bytes()
            })
            .sum();

        let expected_budget_ceiling = max_bytes.max(protected_minimum_bytes);
        prop_assert!(
            retained_total_bytes <= expected_budget_ceiling,
            "Retained bytes {} exceeds expected budget ceiling {}",
            retained_total_bytes, expected_budget_ceiling
        );

        // 3. Freed bytes accuracy
        let calculated_freed: u64 = items
            .iter()
            .filter(|i| evicted_set.contains(i.id()))
            .map(|i| i.size_bytes())
            .sum();
        prop_assert_eq!(plan.freed_bytes, calculated_freed);
    }

    #[test]
    fn keep_last_preserves_stream_invariant(
        stream_count in 1..4usize,
        item_count in 1..25usize,
        n in 0..10usize,
    ) {
        let mut items = Vec::new();
        for i in 0..item_count {
            let s_idx = i % stream_count;
            items.push(TestItem {
                id: format!("item_{}", i),
                stream_id: format!("stream_{}", s_idx),
                tier: DegradationTier::Tier1HwSurface,
                timestamp: 1000 + (i as u64),
                last_accessed: 1000 + (i as u64),
                size_bytes: 1000,
            });
        }

        let policy = RetentionPolicy::KeepLast { n };
        let plan = RetentionEngine::plan_eviction(&items, &policy, 5000);
        let evicted_set: HashSet<&str> = plan.evict_ids.iter().map(|s| s.as_str()).collect();

        let mut retained_per_stream: HashSet<&str> = HashSet::new();
        for item in &items {
            if !evicted_set.contains(item.id()) {
                retained_per_stream.insert(item.stream_id());
            }
        }

        let all_streams: HashSet<&str> = items.iter().map(|i| i.stream_id()).collect();
        for stream in &all_streams {
            prop_assert!(retained_per_stream.contains(stream));
        }
    }

    #[test]
    fn ttl_preserves_stream_invariant(
        stream_count in 1..4usize,
        item_count in 1..25usize,
        ttl_secs in 1..100u64,
    ) {
        let mut items = Vec::new();
        for i in 0..item_count {
            let s_idx = i % stream_count;
            items.push(TestItem {
                id: format!("item_{}", i),
                stream_id: format!("stream_{}", s_idx),
                tier: DegradationTier::Tier1HwSurface,
                timestamp: 100 + (i as u64 * 10),
                last_accessed: 100 + (i as u64 * 10),
                size_bytes: 1000,
            });
        }

        let policy = RetentionPolicy::TimeToLive { ttl_secs };
        let plan = RetentionEngine::plan_eviction(&items, &policy, 1000);
        let evicted_set: HashSet<&str> = plan.evict_ids.iter().map(|s| s.as_str()).collect();

        let mut retained_per_stream: HashSet<&str> = HashSet::new();
        for item in &items {
            if !evicted_set.contains(item.id()) {
                retained_per_stream.insert(item.stream_id());
            }
        }

        let all_streams: HashSet<&str> = items.iter().map(|i| i.stream_id()).collect();
        for stream in &all_streams {
            prop_assert!(retained_per_stream.contains(stream));
        }
    }
}
