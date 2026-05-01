//! Cache breakpoints for Anthropic prompt caching.
//!
//! # Where to place breakpoints
//!
//! A `cache_control: ephemeral` marker tells the server "cache the request
//! up to and including this block." Typical strategies:
//!
//! - **Cache system + tools + first user turn.** The longest-lived prefix.
//!   Use [`CreateMessageRequestBuilder::cache_control_on_system`],
//!   [`CreateMessageRequestBuilder::cache_control_on_tools`], and either
//!   [`CreateMessageRequestBuilder::cache_control_on_last_user`] (called
//!   once before the first send) or [`ContentBlock::text_cached`] to mark
//!   that user turn at construction time.
//! - **Refresh on each turn.** For long conversations, place a fresh
//!   ephemeral breakpoint on the most recent user turn each request.
//!   `Conversation::with_auto_cache(AutoCacheMode::SystemAndLastUser)`
//!   does this for you.
//!
//! TTL choice: the default 5-minute cache is right for nearly all
//! interactive workloads. `"1h"` requires the
//! `extended-cache-ttl-2025-04-11` beta header and is meant for batch /
//! long-running pipelines where the same prefix sees sustained traffic.
//!
//! [`CreateMessageRequestBuilder::cache_control_on_system`]:
//! crate::messages::request::CreateMessageRequestBuilder::cache_control_on_system
//! [`CreateMessageRequestBuilder::cache_control_on_tools`]:
//! crate::messages::request::CreateMessageRequestBuilder::cache_control_on_tools
//! [`CreateMessageRequestBuilder::cache_control_on_last_user`]:
//! crate::messages::request::CreateMessageRequestBuilder::cache_control_on_last_user
//! [`ContentBlock::text_cached`]:
//! crate::messages::content::ContentBlock::text_cached

use serde::{Deserialize, Serialize};

/// Marks a cache breakpoint on a content block, system prompt, or tool definition.
///
/// Currently only `Ephemeral` is supported. The `ttl` field is optional and
/// defaults server-side to `"5m"`; `"1h"` requires the appropriate beta header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CacheControl {
    /// Ephemeral cache entry.
    Ephemeral {
        /// Cache TTL hint, e.g. `"5m"` or `"1h"`. Server applies a default if omitted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ttl: Option<String>,
    },
}

impl CacheControl {
    /// Convenience constructor for an ephemeral cache breakpoint with the default TTL.
    pub fn ephemeral() -> Self {
        Self::Ephemeral { ttl: None }
    }

    /// Convenience constructor for an ephemeral cache breakpoint with a specific TTL.
    pub fn ephemeral_ttl(ttl: impl Into<String>) -> Self {
        Self::Ephemeral {
            ttl: Some(ttl.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn ephemeral_round_trips_without_ttl() {
        let cc = CacheControl::ephemeral();
        let v = serde_json::to_value(&cc).unwrap();
        assert_eq!(v, json!({"type": "ephemeral"}));
        let parsed: CacheControl = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, cc);
    }

    #[test]
    fn ephemeral_round_trips_with_ttl() {
        let cc = CacheControl::ephemeral_ttl("1h");
        let v = serde_json::to_value(&cc).unwrap();
        assert_eq!(v, json!({"type": "ephemeral", "ttl": "1h"}));
        let parsed: CacheControl = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, cc);
    }
}
