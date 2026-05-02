//! Response types returned by the Messages API.
//!
//! | Type | Source |
//! |---|---|
//! | [`Message`] | `POST /v1/messages` (non-streaming) or [`EventStream::aggregate`](crate::messages::stream::EventStream::aggregate) |
//! | [`CountTokensResponse`] | `POST /v1/messages/count_tokens` |
//! | [`ContainerInfo`] | Nested in `Message::container` when code-execution containers are active |
//!
//! `Message.content` is `Vec<ContentBlock>` -- iterate it with a match on
//! [`crate::messages::ContentBlock::Known`] to extract text, tool calls, etc.

use serde::{Deserialize, Serialize};

use crate::forward_compat::dispatch_known_or_other;
use crate::messages::content::ContentBlock;
use crate::types::{ModelId, Role, StopReason, Usage};

/// A complete (non-streaming) Messages-API response.
///
/// Usually produced by the SDK from a wire payload rather than built by
/// hand. Tests that need a fixture should round-trip a JSON literal through
/// [`serde_json::from_value`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Message {
    /// Unique message identifier (e.g. `msg_01ABC...`).
    pub id: String,
    /// Wire `type` discriminant. Always `"message"` for non-streaming responses;
    /// retained on the struct for full round-trip fidelity.
    #[serde(rename = "type", default = "default_message_kind")]
    pub kind: String,
    /// Author of the message. Always [`Role::Assistant`] for responses.
    #[serde(default = "default_assistant_role")]
    pub role: Role,
    /// Ordered list of content blocks the model produced.
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    /// The model that produced this response.
    pub model: ModelId,
    /// Why the model stopped, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// The stop sequence that triggered termination, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    /// Structured information about *why* the model stopped (e.g. for
    /// `refusal` stops, the policy category and an explanation). `None`
    /// when no extra detail is reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_details: Option<StopDetails>,
    /// Token usage and related counters.
    #[serde(default)]
    pub usage: Usage,
    /// Context-management edits applied to the request (e.g. trimmed
    /// thinking blocks or tool-use history). Present only when a
    /// context-management strategy was active and triggered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_management: Option<ResponseContextManagement>,
    /// Container metadata, present when the request used the code-execution
    /// container tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerInfo>,
}

// ---------------------------------------------------------------------------
// stop_details
// ---------------------------------------------------------------------------

/// Structured information about why the model stopped.
///
/// Forward-compatible: unknown `type` tags deserialize into
/// [`StopDetails::Other`] with the raw JSON preserved byte-for-byte.
/// Currently only one variant (`Refusal`) is known; new variants may appear
/// in future API versions.
#[derive(Debug, Clone, PartialEq)]
pub enum StopDetails {
    /// A stop-details payload whose `type` is recognized.
    Known(KnownStopDetails),
    /// A stop-details payload whose `type` is not recognized; raw JSON
    /// preserved.
    Other(serde_json::Value),
}

/// All stop-details variants known to this SDK version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnownStopDetails {
    /// The model stopped because it refused the request on policy grounds.
    Refusal(RefusalStopDetails),
}

/// Policy-refusal stop details.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RefusalStopDetails {
    /// The policy category that triggered the refusal (`"cyber"`, `"bio"`,
    /// or `None` when the refusal doesn't map to a named category).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Human-readable explanation of the refusal. Not guaranteed to be
    /// stable; `None` when no explanation is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

const KNOWN_STOP_DETAILS_TAGS: &[&str] = &["refusal"];

impl Serialize for StopDetails {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            StopDetails::Known(k) => k.serialize(s),
            StopDetails::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for StopDetails {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        dispatch_known_or_other(
            raw,
            KNOWN_STOP_DETAILS_TAGS,
            StopDetails::Known,
            StopDetails::Other,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl From<KnownStopDetails> for StopDetails {
    fn from(k: KnownStopDetails) -> Self {
        StopDetails::Known(k)
    }
}

// ---------------------------------------------------------------------------
// context_management
// ---------------------------------------------------------------------------

/// Context-management edits applied during the request.
///
/// Present when a context-management strategy (e.g. `compact_20260112`,
/// `clear_thinking_20251015`) was active and triggered. Each edit in
/// `applied_edits` is forward-compatible: unknown `type` tags fall through
/// to [`ContextEdit::Other`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResponseContextManagement {
    /// The edits applied, in order.
    #[serde(default)]
    pub applied_edits: Vec<ContextEdit>,
}

/// One context-management edit.
///
/// Forward-compatible: unknown `type` tags deserialize into
/// [`ContextEdit::Other`] with the raw JSON preserved byte-for-byte.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextEdit {
    /// An edit whose `type` is recognized.
    Known(KnownContextEdit),
    /// An edit whose `type` is not recognized; raw JSON preserved.
    Other(serde_json::Value),
}

/// All context-edit variants known to this SDK version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnownContextEdit {
    /// Cleared one or more thinking blocks from the history.
    #[serde(rename = "clear_thinking_20251015")]
    ClearThinking(ClearThinkingEdit),
    /// Cleared one or more tool-use / tool-result pairs from the history.
    #[serde(rename = "clear_tool_uses_20250919")]
    ClearToolUses(ClearToolUsesEdit),
}

/// Details for a `clear_thinking_20251015` context-management edit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClearThinkingEdit {
    /// Number of input tokens cleared by this edit.
    pub cleared_input_tokens: u64,
    /// Number of thinking turns that were cleared.
    pub cleared_thinking_turns: u64,
}

/// Details for a `clear_tool_uses_20250919` context-management edit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClearToolUsesEdit {
    /// Number of input tokens cleared by this edit.
    pub cleared_input_tokens: u64,
    /// Number of tool-use/tool-result pairs that were cleared.
    pub cleared_tool_uses: u64,
}

const KNOWN_CONTEXT_EDIT_TAGS: &[&str] = &["clear_thinking_20251015", "clear_tool_uses_20250919"];

impl Serialize for ContextEdit {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ContextEdit::Known(k) => k.serialize(s),
            ContextEdit::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ContextEdit {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        dispatch_known_or_other(
            raw,
            KNOWN_CONTEXT_EDIT_TAGS,
            ContextEdit::Known,
            ContextEdit::Other,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl From<KnownContextEdit> for ContextEdit {
    fn from(k: KnownContextEdit) -> Self {
        ContextEdit::Known(k)
    }
}

// ---------------------------------------------------------------------------

fn default_message_kind() -> String {
    "message".to_owned()
}

fn default_assistant_role() -> Role {
    Role::Assistant
}

/// Container metadata returned when a request used the code-execution
/// container tool.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ContainerInfo {
    /// Container identifier.
    pub id: String,
    /// Container expiration timestamp (ISO-8601), if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Response payload for `POST /v1/messages/count_tokens`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CountTokensResponse {
    /// Number of input tokens the request would consume.
    pub input_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::content::KnownBlock;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn realistic_message_response_round_trips() {
        let raw = json!({
            "id": "msg_01ABCDEF",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello!"}
            ],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let msg: Message = serde_json::from_value(raw).expect("deserialize");
        assert_eq!(msg.id, "msg_01ABCDEF");
        assert_eq!(msg.kind, "message");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.model, ModelId::SONNET_4_6);
        assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(msg.usage.input_tokens, 10);
        assert_eq!(msg.usage.output_tokens, 5);
        assert_eq!(msg.content.len(), 1);
        assert_eq!(msg.content[0].type_tag(), Some("text"));

        let reserialized = serde_json::to_value(&msg).expect("serialize");
        let parsed_again: Message = serde_json::from_value(reserialized).expect("re-deserialize");
        assert_eq!(parsed_again, msg, "round-trip mismatch");
    }

    #[test]
    fn message_with_unknown_content_block_round_trips() {
        let raw = json!({
            "id": "msg_X",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "hi"},
                {"type": "future_block", "payload": 42}
            ],
            "model": "claude-opus-4-7",
            "usage": {"input_tokens": 1, "output_tokens": 1}
        });

        let msg: Message = serde_json::from_value(raw.clone()).expect("deserialize");
        assert_eq!(msg.content.len(), 2);
        assert_eq!(msg.content[0].type_tag(), Some("text"));
        assert_eq!(msg.content[1].type_tag(), Some("future_block"));
        assert!(msg.content[1].other().is_some());

        // Reserializing must put the unknown block back byte-for-byte.
        let reserialized = serde_json::to_value(&msg).expect("serialize");
        let blocks = reserialized.get("content").unwrap().as_array().unwrap();
        assert_eq!(blocks[1], json!({"type": "future_block", "payload": 42}));
    }

    #[test]
    fn message_kind_defaults_when_missing() {
        // A wire payload missing the `type` field still parses, with kind defaulting to "message".
        let raw = json!({
            "id": "msg_1",
            "role": "assistant",
            "content": [],
            "model": "claude-sonnet-4-6",
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        let msg: Message = serde_json::from_value(raw).expect("deserialize");
        assert_eq!(msg.kind, "message");
    }

    #[test]
    fn message_with_tool_use_block_round_trips() {
        let msg = Message {
            id: "msg_tool".into(),
            kind: "message".into(),
            role: Role::Assistant,
            content: vec![ContentBlock::Known(KnownBlock::ToolUse {
                id: "toolu_1".into(),
                name: "lookup".into(),
                input: json!({"q": "rust"}),
            })],
            model: ModelId::HAIKU_4_5,
            stop_reason: Some(StopReason::ToolUse),
            stop_sequence: None,
            stop_details: None,
            usage: Usage {
                input_tokens: 7,
                output_tokens: 3,
                ..Usage::default()
            },
            context_management: None,
            container: None,
        };

        let v = serde_json::to_value(&msg).expect("serialize");
        let parsed: Message = serde_json::from_value(v).expect("deserialize");
        assert_eq!(parsed, msg);
    }

    #[test]
    fn stop_details_refusal_round_trips() {
        let raw = json!({
            "type": "refusal",
            "category": "cyber",
            "explanation": "Request involves offensive cyber techniques."
        });
        let sd: StopDetails = serde_json::from_value(raw.clone()).unwrap();
        match &sd {
            StopDetails::Known(KnownStopDetails::Refusal(r)) => {
                assert_eq!(r.category.as_deref(), Some("cyber"));
                assert!(r.explanation.is_some());
            }
            other => panic!("expected Refusal, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(&sd).unwrap(), raw);
    }

    #[test]
    fn stop_details_null_category_round_trips() {
        let raw = json!({"type": "refusal", "category": null, "explanation": null});
        let sd: StopDetails = serde_json::from_value(raw).unwrap();
        if let StopDetails::Known(KnownStopDetails::Refusal(r)) = &sd {
            assert!(r.category.is_none());
        } else {
            panic!("expected Refusal");
        }
    }

    #[test]
    fn stop_details_unknown_type_falls_through_to_other() {
        let raw = json!({"type": "future_stop_reason", "detail": 42});
        let sd: StopDetails = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(sd, StopDetails::Other(_)));
        assert_eq!(serde_json::to_value(&sd).unwrap(), raw);
    }

    #[test]
    fn context_edit_clear_thinking_round_trips() {
        let raw = json!({
            "type": "clear_thinking_20251015",
            "cleared_input_tokens": 1500,
            "cleared_thinking_turns": 3
        });
        let edit: ContextEdit = serde_json::from_value(raw.clone()).unwrap();
        match &edit {
            ContextEdit::Known(KnownContextEdit::ClearThinking(e)) => {
                assert_eq!(e.cleared_input_tokens, 1500);
                assert_eq!(e.cleared_thinking_turns, 3);
            }
            other => panic!("expected ClearThinking, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(&edit).unwrap(), raw);
    }

    #[test]
    fn context_edit_clear_tool_uses_round_trips() {
        let raw = json!({
            "type": "clear_tool_uses_20250919",
            "cleared_input_tokens": 800,
            "cleared_tool_uses": 2
        });
        let edit: ContextEdit = serde_json::from_value(raw.clone()).unwrap();
        if let ContextEdit::Known(KnownContextEdit::ClearToolUses(e)) = &edit {
            assert_eq!(e.cleared_tool_uses, 2);
        } else {
            panic!("expected ClearToolUses");
        }
        assert_eq!(serde_json::to_value(&edit).unwrap(), raw);
    }

    #[test]
    fn context_edit_unknown_type_falls_through_to_other() {
        let raw = json!({"type": "compact_20260112", "summary": "..."});
        let edit: ContextEdit = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(edit, ContextEdit::Other(_)));
        assert_eq!(serde_json::to_value(&edit).unwrap(), raw);
    }

    #[test]
    fn response_context_management_round_trips() {
        let raw = json!({
            "applied_edits": [
                {"type": "clear_thinking_20251015", "cleared_input_tokens": 500, "cleared_thinking_turns": 1},
                {"type": "clear_tool_uses_20250919", "cleared_input_tokens": 200, "cleared_tool_uses": 1}
            ]
        });
        let cm: ResponseContextManagement = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(cm.applied_edits.len(), 2);
        assert!(matches!(
            &cm.applied_edits[0],
            ContextEdit::Known(KnownContextEdit::ClearThinking(_))
        ));
        assert_eq!(serde_json::to_value(&cm).unwrap(), raw);
    }

    #[test]
    fn message_with_stop_details_and_context_management_round_trips() {
        let raw = json!({
            "id": "msg_refusal",
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": "claude-sonnet-4-6",
            "stop_reason": "refusal",
            "usage": {"input_tokens": 5, "output_tokens": 0},
            "stop_details": {"type": "refusal", "category": "bio", "explanation": "Biosecurity policy."},
            "context_management": {
                "applied_edits": [
                    {"type": "clear_thinking_20251015", "cleared_input_tokens": 300, "cleared_thinking_turns": 2}
                ]
            }
        });
        let msg: Message = serde_json::from_value(raw).unwrap();
        assert!(msg.stop_details.is_some());
        assert!(msg.context_management.is_some());
        let cm = msg.context_management.as_ref().unwrap();
        assert_eq!(cm.applied_edits.len(), 1);
    }

    #[test]
    fn count_tokens_response_round_trips() {
        let r = CountTokensResponse { input_tokens: 42 };
        let v = serde_json::to_value(&r).expect("serialize");
        assert_eq!(v, json!({"input_tokens": 42}));
        let parsed: CountTokensResponse = serde_json::from_value(v).expect("deserialize");
        assert_eq!(parsed, r);
    }

    #[test]
    fn container_info_round_trips() {
        let c = ContainerInfo {
            id: "cnt_01".into(),
            expires_at: Some("2026-01-01T00:00:00Z".into()),
        };
        let v = serde_json::to_value(&c).expect("serialize");
        assert_eq!(
            v,
            json!({"id": "cnt_01", "expires_at": "2026-01-01T00:00:00Z"})
        );
        let parsed: ContainerInfo = serde_json::from_value(v).expect("deserialize");
        assert_eq!(parsed, c);
    }

    #[test]
    fn message_with_container_round_trips() {
        let raw = json!({
            "id": "msg_with_container",
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": "claude-opus-4-7",
            "usage": {"input_tokens": 0, "output_tokens": 0},
            "container": {"id": "cnt_42"}
        });
        let msg: Message = serde_json::from_value(raw).expect("deserialize");
        assert_eq!(msg.container.as_ref().unwrap().id, "cnt_42");
    }
}
