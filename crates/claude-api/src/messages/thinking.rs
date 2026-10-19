//! Configuration for extended thinking.
//!
//! Pass a [`ThinkingConfig`] value in the request to have the model emit
//! `thinking` blocks before its final answer. The modes:
//!
//! - [`ThinkingConfig::Adaptive`] lets the model decide how much to think.
//!   This is the thinking mode for Claude Opus 4.6 and every later model;
//!   Opus 4.7+, Opus 5.x, Sonnet 5.x, Haiku 5.5 and Fable 5.x reject
//!   `budget_tokens` with HTTP 400. Its optional [`ThinkingDisplay`]
//!   controls whether thinking text is returned.
//! - [`ThinkingConfig::Enabled`] caps reasoning at an explicit `budget_tokens`
//!   (counted against `max_tokens`). For Claude Haiku 4.5 and older models.
//! - [`ThinkingConfig::BetweenTools`] turns thinking off on Claude Sonnet
//!   5.5, which rejects [`ThinkingConfig::Disabled`].
//! - [`ThinkingConfig::Disabled`] turns thinking off where the model allows
//!   it. Claude Opus 5.5, Sonnet 5.5 and Fable 5.x reject it.
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
    /// `max_tokens`. For Claude Haiku 4.5 and older models; Opus 4.6 and
    /// Sonnet 4.6 still accept it (deprecated), and later models reject it
    /// with HTTP 400 -- use [`ThinkingConfig::Adaptive`] there instead.
    Enabled {
        /// Maximum tokens the model may spend thinking on this turn.
        budget_tokens: u32,
    },
    /// Enable extended thinking and let the model decide how much to think.
    ///
    /// Serializes to `{"type": "adaptive"}`, plus `"display"` when set.
    Adaptive {
        /// Whether thinking text is returned. `None` leaves the model's
        /// default, which is [`ThinkingDisplay::Omitted`] on Claude Opus 4.7
        /// and later: `thinking` blocks arrive with empty text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<ThinkingDisplay>,
    },
    /// Think only between tool calls, not before the first response text.
    ///
    /// The way to turn thinking off on Claude Sonnet 5.5. Accepted only at
    /// effort `high` or below, and takes no other fields.
    BetweenTools,
    /// Disable extended thinking explicitly.
    Disabled,
}

/// How thinking content is returned on [`ThinkingConfig::Adaptive`].
///
/// Thinking happens and is billed the same under every setting; this only
/// controls what is sent back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ThinkingDisplay {
    /// Return a readable summary of the reasoning.
    Summarized,
    /// Return `thinking` blocks with empty text.
    Omitted,
    /// Hide reasoning but return short progress notes between tool calls.
    /// Beta: requires the `thinking-display-updates-2026-08-18` header.
    Updates,
    /// A display value this SDK does not know.
    #[serde(other)]
    Other,
}

impl ThinkingConfig {
    /// Convenience constructor for the [`ThinkingConfig::Enabled`] variant.
    #[must_use]
    pub fn enabled(budget_tokens: u32) -> Self {
        Self::Enabled { budget_tokens }
    }

    /// Adaptive thinking with the model's default display.
    #[must_use]
    pub fn adaptive() -> Self {
        Self::Adaptive { display: None }
    }

    /// Adaptive thinking with an explicit [`ThinkingDisplay`].
    #[must_use]
    pub fn adaptive_with_display(display: ThinkingDisplay) -> Self {
        Self::Adaptive {
            display: Some(display),
        }
    }

    /// Adaptive thinking that returns summarized thinking text. Use this
    /// when streaming reasoning to a reader; the default display on recent
    /// models returns empty thinking blocks.
    #[must_use]
    pub fn adaptive_summarized() -> Self {
        Self::adaptive_with_display(ThinkingDisplay::Summarized)
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
    fn adaptive_with_display_round_trips() {
        for (display, wire) in [
            (ThinkingDisplay::Summarized, "summarized"),
            (ThinkingDisplay::Omitted, "omitted"),
            (ThinkingDisplay::Updates, "updates"),
        ] {
            let c = ThinkingConfig::adaptive_with_display(display);
            let v = serde_json::to_value(c).unwrap();
            assert_eq!(v, json!({"type": "adaptive", "display": wire}));
            let parsed: ThinkingConfig = serde_json::from_value(v).unwrap();
            assert_eq!(parsed, c);
        }
    }

    #[test]
    fn unknown_display_deserializes_to_other() {
        let parsed: ThinkingConfig =
            serde_json::from_value(json!({"type": "adaptive", "display": "future"})).unwrap();
        assert_eq!(
            parsed,
            ThinkingConfig::adaptive_with_display(ThinkingDisplay::Other)
        );
    }

    #[test]
    fn between_tools_round_trips() {
        let c = ThinkingConfig::BetweenTools;
        let v = serde_json::to_value(c).unwrap();
        assert_eq!(v, json!({"type": "between_tools"}));
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
