//! `CostPreview` -- estimate the USD cost of a request before sending it.
//!
//! The input side is exact: it hits `/v1/messages/count_tokens` to get the
//! tokenizer's actual count. The output side is bounded: we use
//! `request.max_tokens` as the upper bound, since the actual number of
//! output tokens is unknown until generation finishes. Use
//! [`CostPreview::cost_for`] for a point estimate at any specific output
//! count.
//!
//! Obtain via [`Messages::cost_preview`](crate::messages::Messages::cost_preview).

#![cfg(all(feature = "async", feature = "pricing"))]

use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use crate::pricing::PricingTable;
use crate::types::{ModelId, Usage};

/// Pre-flight cost estimate for a request.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct CostPreview {
    /// Model the estimate was computed for.
    pub model: ModelId,
    /// Server-counted input tokens (from `/v1/messages/count_tokens`).
    pub input_tokens: u32,
    /// Output upper bound, taken from `request.max_tokens`.
    pub max_output_tokens: u32,
    /// USD cost of the input tokens alone.
    pub input_cost_usd: f64,
    /// USD cost if the model emits exactly `max_output_tokens` output tokens.
    pub max_output_cost_usd: f64,
    /// `input_cost_usd + max_output_cost_usd`. The largest amount this
    /// request could cost in vanilla usage (excludes cache/server-tool
    /// charges since those are runtime-determined).
    pub max_total_usd: f64,
}

impl CostPreview {
    /// USD cost if the model produces exactly `output_tokens` tokens. Useful
    /// for plotting expected cost against an empirical output-size estimate.
    #[must_use]
    pub fn cost_for(&self, output_tokens: u32, pricing: &PricingTable) -> f64 {
        pricing.cost(
            &self.model,
            &Usage {
                input_tokens: self.input_tokens,
                output_tokens,
                ..Usage::default()
            },
        )
    }
}

/// Bounded cache for `count_tokens` results, keyed by a stable hash of the
/// request body. Use to skip the network round-trip on repeated previews
/// against unchanged inputs (long-running agent sessions, IDE
/// integrations, etc.).
///
/// Eviction is FIFO once `capacity` is reached -- not a true LRU, but
/// adequate for the common pattern of "preview the same prompt many
/// times, occasionally see a new one." Wrap in [`std::sync::Arc`] to
/// share across tasks; the inner state is [`Mutex`]-protected.
#[derive(Debug)]
pub struct CountTokensCache {
    inner: Mutex<CacheInner>,
    capacity: usize,
}

#[derive(Debug)]
struct CacheInner {
    map: HashMap<u64, u32>,
    order: VecDeque<u64>,
}

impl CountTokensCache {
    /// Build a new cache with the given capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                map: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
            }),
            capacity,
        }
    }

    /// Number of entries currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    /// `true` when no entries are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().map.is_empty()
    }

    /// Drop all cached entries.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.map.clear();
        inner.order.clear();
    }

    /// Look up a cached input-token count by request-hash.
    #[must_use]
    pub fn get(&self, key: u64) -> Option<u32> {
        self.lock().map.get(&key).copied()
    }

    /// Insert (or replace) an entry. Evicts the oldest entry by insertion
    /// order if the cache is at capacity and `key` is not already
    /// present.
    // map_entry: the entry API would require disjoint borrows of two
    // fields through a MutexGuard, which DerefMut can't express.
    #[allow(clippy::map_entry)]
    pub fn put(&self, key: u64, value: u32) {
        let mut inner = self.lock();
        if inner.map.contains_key(&key) {
            inner.map.insert(key, value);
            return;
        }
        if inner.order.len() >= self.capacity {
            if let Some(oldest) = inner.order.pop_front() {
                inner.map.remove(&oldest);
            }
        }
        inner.map.insert(key, value);
        inner.order.push_back(key);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CacheInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Hash a value's serde-JSON serialization to a stable u64. Suitable as a
/// cache key for [`CountTokensCache`]. Returns the empty-string hash on
/// serialization failure (effectively groups malformed inputs together;
/// shouldn't happen for crate-owned types).
#[must_use]
pub fn hash_request<T: serde::Serialize>(value: &T) -> u64 {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_put_and_get_round_trips() {
        let cache = CountTokensCache::new(4);
        cache.put(1, 100);
        cache.put(2, 200);
        assert_eq!(cache.get(1), Some(100));
        assert_eq!(cache.get(2), Some(200));
        assert_eq!(cache.get(3), None);
    }

    #[test]
    fn cache_evicts_oldest_when_full() {
        let cache = CountTokensCache::new(2);
        cache.put(1, 10);
        cache.put(2, 20);
        cache.put(3, 30); // evicts 1
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(2), Some(20));
        assert_eq!(cache.get(3), Some(30));
    }

    #[test]
    fn cache_replace_does_not_change_eviction_order() {
        let cache = CountTokensCache::new(2);
        cache.put(1, 10);
        cache.put(2, 20);
        cache.put(1, 11); // replace; should not push 1 to the back
        cache.put(3, 30); // evicts 1 (oldest by insertion)
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(2), Some(20));
        assert_eq!(cache.get(3), Some(30));
    }

    #[test]
    fn cache_clear_drops_all_entries() {
        let cache = CountTokensCache::new(2);
        cache.put(1, 10);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn hash_request_is_stable_across_calls() {
        let v = serde_json::json!({"a": 1, "b": [1, 2, 3]});
        assert_eq!(hash_request(&v), hash_request(&v));
    }

    #[test]
    fn hash_request_distinguishes_different_payloads() {
        let a = serde_json::json!({"a": 1});
        let b = serde_json::json!({"a": 2});
        assert_ne!(hash_request(&a), hash_request(&b));
    }
}
