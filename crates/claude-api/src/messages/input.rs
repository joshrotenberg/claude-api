//! Request-side message structures: [`MessageInput`], [`MessageContent`],
//! [`SystemPrompt`].
//!
//! [`MessageInput`] is one turn in the conversation history sent to the API.
//! Build via [`MessageInput::user`] / [`MessageInput::assistant`] for the
//! common cases, or construct [`MessageContent::Blocks`] directly when you
//! need multiple content blocks in one turn.
//!
//! [`SystemPrompt`] wraps the system string with optional per-block cache
//! breakpoints for prompt caching.

use serde::{Deserialize, Serialize};

use crate::messages::content::ContentBlock;
use crate::types::Role;

/// One turn in the conversation history sent to the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessageInput {
    /// Author of the turn.
    pub role: Role,
    /// Body of the turn.
    pub content: MessageContent,
}

impl MessageInput {
    /// A user-authored turn.
    pub fn user(content: impl Into<MessageContent>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// An assistant-authored turn (used to seed prefill).
    pub fn assistant(content: impl Into<MessageContent>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Content body of a request-side message: either a plain string or a
/// sequence of [`ContentBlock`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Plain text content.
    Text(String),
    /// Structured content composed of multiple blocks (text + image, etc.).
    Blocks(Vec<ContentBlock>),
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<Vec<ContentBlock>> for MessageContent {
    fn from(v: Vec<ContentBlock>) -> Self {
        Self::Blocks(v)
    }
}

impl From<ContentBlock> for MessageContent {
    fn from(b: ContentBlock) -> Self {
        Self::Blocks(vec![b])
    }
}

/// System prompt passed alongside a Messages request.
///
/// A plain string is the common case. The `Blocks` variant lets you apply
/// `cache_control` to specific spans of the system prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemPrompt {
    /// Single-string system prompt.
    Text(String),
    /// Multi-block system prompt with optional per-block cache breakpoints.
    Blocks(Vec<ContentBlock>),
}

impl From<&str> for SystemPrompt {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}

impl From<String> for SystemPrompt {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<Vec<ContentBlock>> for SystemPrompt {
    fn from(v: Vec<ContentBlock>) -> Self {
        Self::Blocks(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn message_input_user_with_string_content() {
        let m = MessageInput::user("hello");
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"role": "user", "content": "hello"})
        );
    }

    #[test]
    fn message_input_assistant_with_blocks_content() {
        let m = MessageInput::assistant(vec![ContentBlock::text("hi")]);
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "hi"}]
            })
        );
    }

    #[test]
    fn message_content_round_trips_text_and_blocks() {
        let t: MessageContent = "x".into();
        assert_eq!(serde_json::to_value(&t).unwrap(), json!("x"));

        let b: MessageContent = ContentBlock::text("y").into();
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            json!([{"type": "text", "text": "y"}])
        );
    }

    #[test]
    fn system_prompt_text_serializes_as_string() {
        let s: SystemPrompt = "be concise".into();
        assert_eq!(serde_json::to_value(&s).unwrap(), json!("be concise"));
    }

    #[test]
    fn system_prompt_blocks_serializes_as_array() {
        let s: SystemPrompt = vec![ContentBlock::text("be concise")].into();
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            json!([{"type": "text", "text": "be concise"}])
        );
    }
}
