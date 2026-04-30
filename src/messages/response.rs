//! Response types: [`Message`], [`CountTokensResponse`], [`ContainerInfo`].

use serde::{Deserialize, Serialize};

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
    /// Token usage and related counters.
    #[serde(default)]
    pub usage: Usage,
    /// Container metadata, present when the request used the code-execution
    /// container tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerInfo>,
}

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
            usage: Usage {
                input_tokens: 7,
                output_tokens: 3,
                ..Usage::default()
            },
            container: None,
        };

        let v = serde_json::to_value(&msg).expect("serialize");
        let parsed: Message = serde_json::from_value(v).expect("deserialize");
        assert_eq!(parsed, msg);
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
