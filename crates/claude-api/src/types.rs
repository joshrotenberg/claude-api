//! Foundational shared types used across every API resource.
//!
//! | Type | Purpose |
//! |---|---|
//! | [`ModelId`] | String-newtype for model identifiers; common models as associated constants |
//! | [`Role`] | `user` / `assistant` message role |
//! | [`Usage`] | Token counts returned with every `Message` response |
//! | [`StopReason`] | Why the model stopped generating (`end_turn`, `max_tokens`, `tool_use`, ...) |
//! | [`ServiceTier`] | `standard` / `priority` / `batch` service tier on the response |

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Identifier for a Claude model.
///
/// Stored as a string rather than an enum so a new model release does not
/// require an SDK bump. Common models are exposed as associated constants
/// for ergonomics; arbitrary values can be constructed with [`ModelId::custom`].
///
/// ```
/// use claude_api::types::ModelId;
///
/// let known = ModelId::SONNET_4_6;
/// let custom = ModelId::custom("claude-some-future-model");
/// assert_eq!(known.as_str(), "claude-sonnet-4-6");
/// assert_eq!(custom.as_str(), "claude-some-future-model");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId(Cow<'static, str>);

impl ModelId {
    /// Claude Fable 5, the most capable model (top tier above Opus).
    pub const FABLE_5: ModelId = ModelId(Cow::Borrowed("claude-fable-5"));
    /// Claude Opus 4.8.
    pub const OPUS_4_8: ModelId = ModelId(Cow::Borrowed("claude-opus-4-8"));
    /// Claude Opus 4.7.
    pub const OPUS_4_7: ModelId = ModelId(Cow::Borrowed("claude-opus-4-7"));
    /// Claude Sonnet 4.6.
    pub const SONNET_4_6: ModelId = ModelId(Cow::Borrowed("claude-sonnet-4-6"));
    /// Claude Haiku 4.5 (dated snapshot).
    pub const HAIKU_4_5: ModelId = ModelId(Cow::Borrowed("claude-haiku-4-5-20251001"));

    /// Construct a [`ModelId`] from an arbitrary string.
    pub fn custom(s: impl Into<String>) -> Self {
        Self(Cow::Owned(s.into()))
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ModelId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for ModelId {
    fn from(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }
}

impl From<String> for ModelId {
    fn from(s: String) -> Self {
        Self(Cow::Owned(s))
    }
}

impl Serialize for ModelId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ModelId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Self::from)
    }
}

/// Conversation role for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Role {
    /// A user-authored turn.
    User,
    /// A model-authored turn.
    Assistant,
    /// An operator instruction inside `messages`, after the conversation
    /// has started. Supported on Claude Opus 4.8, Opus 5.x, Sonnet 5.5,
    /// Haiku 5.5 and Fable 5.x; it must follow a `user` turn and be either
    /// the last message or followed by an `assistant` turn. Adding one
    /// keeps the cached prefix, where editing the top-level `system`
    /// prompt invalidates it.
    System,
}

/// Why the model stopped producing output.
///
/// New variants may appear over time; unknown values deserialize to
/// [`StopReason::Other`]. The original wire string is not preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of turn.
    EndTurn,
    /// Hit the request's `max_tokens`.
    MaxTokens,
    /// Hit a configured stop sequence.
    StopSequence,
    /// Stopped to emit a tool use; caller should run the tool and continue.
    ToolUse,
    /// Paused mid-turn (e.g. for a server-side tool call to complete).
    PauseTurn,
    /// The model refused to answer.
    Refusal,
    /// An unrecognized stop reason; the SDK is older than the API.
    #[serde(other)]
    Other,
}

/// Service tier reported on a response.
///
/// Mirrors Anthropic's tiered routing. New tiers may appear; unknowns
/// deserialize to [`ServiceTier::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    /// Standard service tier.
    Standard,
    /// Priority service tier.
    Priority,
    /// Batch service tier.
    Batch,
    /// An unrecognized service tier.
    #[serde(other)]
    Other,
}

/// Token usage and related counters returned on every response.
///
/// `#[non_exhaustive]` because Anthropic adds fields here regularly
/// (`cache_creation`, `server_tool_use`, `service_tier` are recent additions).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Usage {
    /// Number of input tokens billed.
    pub input_tokens: u32,
    /// Number of output tokens billed.
    pub output_tokens: u32,
    /// Tokens written to the prompt cache on this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    /// Tokens read from the prompt cache on this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
    /// Per-TTL breakdown of cache writes (5-minute vs 1-hour).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation: Option<CacheCreationBreakdown>,
    /// Counters for server-side tool usage (e.g. web search).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_tool_use: Option<ServerToolUseUsage>,
    /// Service tier the request actually ran on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,
    /// Inference geography the request was routed to (e.g.
    /// `"not_available"`, region codes when reported). Open string for
    /// forward-compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference_geo: Option<String>,
    /// Speed the request ran at (`"standard"` or `"fast"` for fast mode).
    /// Open string for forward-compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    /// Usage fields this SDK does not model yet (for example
    /// `iterations` on server-side fallback), kept so a stored `Usage`
    /// round-trips without loss.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Usage {
    /// Fold a streaming `message_delta` usage into the usage reported at
    /// `message_start`. Token counts in the delta are cumulative and
    /// replace the earlier values; optional fields the delta omits keep
    /// their `message_start` values.
    #[cfg_attr(not(feature = "streaming"), allow(dead_code))]
    pub(crate) fn merge_delta(&mut self, delta: Usage) {
        let Usage {
            input_tokens,
            output_tokens,
            cache_creation_input_tokens,
            cache_read_input_tokens,
            cache_creation,
            server_tool_use,
            service_tier,
            inference_geo,
            speed,
            extra,
        } = delta;
        self.input_tokens = input_tokens;
        self.output_tokens = output_tokens;
        if cache_creation_input_tokens.is_some() {
            self.cache_creation_input_tokens = cache_creation_input_tokens;
        }
        if cache_read_input_tokens.is_some() {
            self.cache_read_input_tokens = cache_read_input_tokens;
        }
        if cache_creation.is_some() {
            self.cache_creation = cache_creation;
        }
        if server_tool_use.is_some() {
            self.server_tool_use = server_tool_use;
        }
        if service_tier.is_some() {
            self.service_tier = service_tier;
        }
        if inference_geo.is_some() {
            self.inference_geo = inference_geo;
        }
        if speed.is_some() {
            self.speed = speed;
        }
        self.extra.extend(extra);
    }
}

/// Per-TTL breakdown of cache-creation tokens.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CacheCreationBreakdown {
    /// Tokens written to a 5-minute-TTL cache entry.
    #[serde(default)]
    pub ephemeral_5m_input_tokens: u32,
    /// Tokens written to a 1-hour-TTL cache entry.
    #[serde(default)]
    pub ephemeral_1h_input_tokens: u32,
}

/// Counters for server-side tool invocations billed on this request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ServerToolUseUsage {
    /// Number of web-search requests issued.
    #[serde(default)]
    pub web_search_requests: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::de::DeserializeOwned;

    fn round_trip<T>(value: &T, expected_json: &str)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        assert_eq!(json, expected_json, "serialized form mismatch");
        let parsed: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&parsed, value, "round-trip mismatch");
    }

    #[test]
    fn model_id_serializes_as_string() {
        round_trip(&ModelId::FABLE_5, "\"claude-fable-5\"");
        round_trip(&ModelId::OPUS_4_8, "\"claude-opus-4-8\"");
        round_trip(&ModelId::OPUS_4_7, "\"claude-opus-4-7\"");
        round_trip(&ModelId::SONNET_4_6, "\"claude-sonnet-4-6\"");
        round_trip(&ModelId::HAIKU_4_5, "\"claude-haiku-4-5-20251001\"");
        round_trip(
            &ModelId::custom("claude-future-foo"),
            "\"claude-future-foo\"",
        );
    }

    #[test]
    fn model_id_const_equals_custom() {
        assert_eq!(ModelId::OPUS_4_7, ModelId::custom("claude-opus-4-7"));
    }

    #[test]
    fn model_id_display_and_as_ref() {
        assert_eq!(ModelId::SONNET_4_6.to_string(), "claude-sonnet-4-6");
        assert_eq!(
            <ModelId as AsRef<str>>::as_ref(&ModelId::SONNET_4_6),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn role_serializes_lowercase() {
        round_trip(&Role::User, "\"user\"");
        round_trip(&Role::Assistant, "\"assistant\"");
    }

    #[test]
    fn stop_reason_round_trips_known_variants() {
        round_trip(&StopReason::EndTurn, "\"end_turn\"");
        round_trip(&StopReason::MaxTokens, "\"max_tokens\"");
        round_trip(&StopReason::StopSequence, "\"stop_sequence\"");
        round_trip(&StopReason::ToolUse, "\"tool_use\"");
        round_trip(&StopReason::PauseTurn, "\"pause_turn\"");
        round_trip(&StopReason::Refusal, "\"refusal\"");
    }

    #[test]
    fn stop_reason_unknown_falls_back_to_other() {
        let parsed: StopReason = serde_json::from_str("\"some_new_reason\"").expect("deserialize");
        assert_eq!(parsed, StopReason::Other);
    }

    #[test]
    fn service_tier_unknown_falls_back_to_other() {
        let parsed: ServiceTier = serde_json::from_str("\"enterprise\"").expect("deserialize");
        assert_eq!(parsed, ServiceTier::Other);
        round_trip(&ServiceTier::Standard, "\"standard\"");
        round_trip(&ServiceTier::Priority, "\"priority\"");
        round_trip(&ServiceTier::Batch, "\"batch\"");
    }

    #[test]
    fn usage_minimal_payload_round_trips() {
        let u = Usage {
            input_tokens: 12,
            output_tokens: 34,
            ..Usage::default()
        };
        round_trip(&u, r#"{"input_tokens":12,"output_tokens":34}"#);
    }

    #[test]
    fn usage_full_payload_round_trips() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(20),
            cache_read_input_tokens: Some(80),
            cache_creation: Some(CacheCreationBreakdown {
                ephemeral_5m_input_tokens: 10,
                ephemeral_1h_input_tokens: 10,
            }),
            server_tool_use: Some(ServerToolUseUsage {
                web_search_requests: 3,
            }),
            service_tier: Some(ServiceTier::Standard),
            inference_geo: Some("us-east-1".into()),
            speed: Some("fast".into()),
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&u).expect("serialize");
        let parsed: Usage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, u);
    }

    #[test]
    fn usage_tolerates_unknown_fields() {
        let json = r#"{
            "input_tokens": 5,
            "output_tokens": 7,
            "future_field": "ignored"
        }"#;
        let parsed: Usage = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.input_tokens, 5);
        assert_eq!(parsed.output_tokens, 7);
    }

    #[test]
    fn usage_keeps_unknown_fields_on_round_trip() {
        let wire = serde_json::json!({
            "input_tokens": 5,
            "output_tokens": 7,
            "speed": "standard",
            "iterations": [{"type": "message", "input_tokens": 5, "output_tokens": 7}]
        });
        let parsed: Usage = serde_json::from_value(wire.clone()).expect("deserialize");
        assert_eq!(parsed.speed.as_deref(), Some("standard"));
        assert!(parsed.extra.contains_key("iterations"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
    }

    #[test]
    fn merge_delta_keeps_fields_the_delta_omits() {
        let mut start: Usage = serde_json::from_value(serde_json::json!({
            "input_tokens": 120,
            "output_tokens": 1,
            "cache_creation_input_tokens": 100,
            "cache_read_input_tokens": 0,
            "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 100},
            "service_tier": "standard",
            "inference_geo": "us"
        }))
        .unwrap();
        let delta: Usage = serde_json::from_value(serde_json::json!({
            "input_tokens": 120,
            "output_tokens": 42,
            "cache_creation_input_tokens": 100,
            "cache_read_input_tokens": 0
        }))
        .unwrap();
        start.merge_delta(delta);
        assert_eq!(start.output_tokens, 42);
        assert_eq!(start.cache_creation.unwrap().ephemeral_1h_input_tokens, 100);
        assert_eq!(start.service_tier, Some(ServiceTier::Standard));
        assert_eq!(start.inference_geo.as_deref(), Some("us"));
    }

    #[test]
    fn role_system_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
        let parsed: Role = serde_json::from_str(r#""system""#).unwrap();
        assert_eq!(parsed, Role::System);
    }
}
