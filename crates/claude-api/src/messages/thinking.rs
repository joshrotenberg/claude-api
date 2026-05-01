//! Configuration for extended thinking.

use serde::{Deserialize, Serialize};

/// Whether and how the model should produce extended-thinking output.
///
/// When [`ThinkingConfig::Enabled`], the model emits one or more
/// [`Thinking`](crate::messages::content::KnownBlock::Thinking) blocks
/// before its final answer. `budget_tokens` caps the thinking length;
/// it counts against `max_tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ThinkingConfig {
    /// Enable extended thinking with a per-turn token budget.
    Enabled {
        /// Maximum tokens the model may spend thinking on this turn.
        budget_tokens: u32,
    },
    /// Disable extended thinking explicitly.
    Disabled,
}

impl ThinkingConfig {
    /// Convenience constructor for the [`ThinkingConfig::Enabled`] variant.
    #[must_use]
    pub fn enabled(budget_tokens: u32) -> Self {
        Self::Enabled { budget_tokens }
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
    fn disabled_round_trips() {
        let c = ThinkingConfig::Disabled;
        let v = serde_json::to_value(c).unwrap();
        assert_eq!(v, json!({"type": "disabled"}));
        let parsed: ThinkingConfig = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }
}
