//! Configuration for extended thinking.
//!
//! Pass a [`ThinkingConfig`] value in the request to have the model emit
//! `thinking` blocks before its final answer. Two modes are available:
//!
//! - [`ThinkingConfig::Adaptive`] lets the model decide how much to think.
//!   This is the only accepted mode on Claude Opus 4.7+ and Fable 5, which
//!   reject `budget_tokens` with HTTP 400.
//! - [`ThinkingConfig::Enabled`] caps reasoning at an explicit `budget_tokens`
//!   (counted against `max_tokens`). Supported on Claude Sonnet 4.6 and
//!   earlier Opus models.
//!
//! See [`crate::models::ModelCapabilities`] to check model support at runtime.

use serde::{Deserialize, Serialize};

/// Whether and how the model should produce extended-thinking output.
///
/// When thinking is on, the model emits one or more
/// [`Thinking`](crate::messages::content::KnownBlock::Thinking) blocks
/// before its final answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ThinkingConfig {
    /// Enable extended thinking with a per-turn token budget.
    ///
    /// `budget_tokens` caps the thinking length and counts against
    /// `max_tokens`. Supported on Claude Sonnet 4.6 and earlier Opus models;
    /// Claude Opus 4.7+ and Fable 5 reject it with HTTP 400 -- use
    /// [`ThinkingConfig::Adaptive`] there instead.
    Enabled {
        /// Maximum tokens the model may spend thinking on this turn.
        budget_tokens: u32,
    },
    /// Enable extended thinking and let the model decide how much to think.
    ///
    /// The only accepted thinking mode on Claude Opus 4.7+ and Fable 5.
    /// Serializes to `{"type": "adaptive"}`.
    Adaptive,
    /// Disable extended thinking explicitly.
    Disabled,
}

impl ThinkingConfig {
    /// Convenience constructor for the [`ThinkingConfig::Enabled`] variant.
    #[must_use]
    pub fn enabled(budget_tokens: u32) -> Self {
        Self::Enabled { budget_tokens }
    }

    /// Convenience constructor for the [`ThinkingConfig::Adaptive`] variant.
    ///
    /// Use this on Claude Opus 4.7+ and Fable 5, which do not accept
    /// `budget_tokens`.
    #[must_use]
    pub fn adaptive() -> Self {
        Self::Adaptive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn enabled_round_trips() {
        let c = ThinkingConfig::enabled(8192);
        let v = serde_json::to_value(c).unwrap();
        assert_eq!(v, json!({"type": "enabled", "budget_tokens": 8192}));
        let parsed: ThinkingConfig = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn adaptive_round_trips() {
        let c = ThinkingConfig::adaptive();
        let v = serde_json::to_value(c).unwrap();
        assert_eq!(v, json!({"type": "adaptive"}));
        let parsed: ThinkingConfig = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn disabled_round_trips() {
        let c = ThinkingConfig::Disabled;
        let v = serde_json::to_value(c).unwrap();
        assert_eq!(v, json!({"type": "disabled"}));
        let parsed: ThinkingConfig = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }
}
