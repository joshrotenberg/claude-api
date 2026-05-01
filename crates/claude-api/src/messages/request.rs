//! Request payloads for the Messages API.
//!
//! [`CreateMessageRequest`] is the typed builder for `POST /v1/messages`.
//! [`CountTokensRequest`] is its slimmer sibling for
//! `POST /v1/messages/count_tokens`.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::messages::cache::CacheControl;
use crate::messages::content::{ContentBlock, KnownBlock};
use crate::messages::input::{MessageInput, SystemPrompt};

fn apply_cache_control_to_last_block_with(blocks: &mut [ContentBlock], cc: CacheControl) {
    let Some(last) = blocks.last_mut() else {
        return;
    };
    if let ContentBlock::Known(
        KnownBlock::Text { cache_control, .. }
        | KnownBlock::Image { cache_control, .. }
        | KnownBlock::Document { cache_control, .. }
        | KnownBlock::ToolResult { cache_control, .. },
    ) = last
    {
        *cache_control = Some(cc);
    }
}
use crate::messages::mcp::McpServerConfig;
use crate::messages::metadata::{MessageMetadata, RequestServiceTier};
use crate::messages::thinking::ThinkingConfig;
use crate::messages::tools::{Tool, ToolChoice};
use crate::types::ModelId;

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// Request payload for `POST /v1/messages`.
///
/// Construct via [`CreateMessageRequest::builder`].
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateMessageRequest {
    /// Model to query.
    pub model: ModelId,
    /// Maximum number of output tokens to generate.
    pub max_tokens: u32,
    /// Conversation history.
    pub messages: Vec<MessageInput>,

    /// Optional system prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Nucleus sampling cutoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Top-k sampling cutoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Custom stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Tools the model may invoke.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    /// Tool-use policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Optional per-request metadata (`user_id`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
    /// Request-side service tier preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<RequestServiceTier>,
    /// Extended-thinking configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// MCP servers exposed to the model on this request.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServerConfig>,
    /// Container ID for the code-execution built-in tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,

    /// Whether to stream the response. Set internally by `create_stream`;
    /// not normally touched by callers.
    #[doc(hidden)]
    #[serde(default, skip_serializing_if = "is_false")]
    pub stream: bool,
}

impl CreateMessageRequest {
    /// Begin configuring a request.
    #[must_use]
    pub fn builder() -> CreateMessageRequestBuilder {
        CreateMessageRequestBuilder::default()
    }
}

/// Builder for [`CreateMessageRequest`].
#[derive(Debug, Default)]
pub struct CreateMessageRequestBuilder {
    model: Option<ModelId>,
    max_tokens: Option<u32>,
    messages: Vec<MessageInput>,
    system: Option<SystemPrompt>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    stop_sequences: Option<Vec<String>>,
    tools: Vec<Tool>,
    tool_choice: Option<ToolChoice>,
    metadata: Option<MessageMetadata>,
    service_tier: Option<RequestServiceTier>,
    thinking: Option<ThinkingConfig>,
    mcp_servers: Vec<McpServerConfig>,
    container: Option<String>,
}

impl CreateMessageRequestBuilder {
    /// Set the model. Required.
    #[must_use]
    pub fn model(mut self, m: impl Into<ModelId>) -> Self {
        self.model = Some(m.into());
        self
    }

    /// Set the max output tokens. Required.
    #[must_use]
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system(mut self, s: impl Into<SystemPrompt>) -> Self {
        self.system = Some(s.into());
        self
    }

    /// Replace the entire conversation history.
    #[must_use]
    pub fn messages(mut self, msgs: Vec<MessageInput>) -> Self {
        self.messages = msgs;
        self
    }

    /// Append a user-authored message to the history.
    #[must_use]
    pub fn user(mut self, content: impl Into<crate::messages::input::MessageContent>) -> Self {
        self.messages.push(MessageInput::user(content));
        self
    }

    /// Append an assistant-authored message (typically used for prefill).
    #[must_use]
    pub fn assistant(mut self, content: impl Into<crate::messages::input::MessageContent>) -> Self {
        self.messages.push(MessageInput::assistant(content));
        self
    }

    /// Set the available tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the tool-use policy.
    #[must_use]
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set the sampling temperature.
    #[must_use]
    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Set the nucleus sampling cutoff.
    #[must_use]
    pub fn top_p(mut self, p: f32) -> Self {
        self.top_p = Some(p);
        self
    }

    /// Set the top-k sampling cutoff.
    #[must_use]
    pub fn top_k(mut self, k: u32) -> Self {
        self.top_k = Some(k);
        self
    }

    /// Set custom stop sequences.
    #[must_use]
    pub fn stop_sequences(mut self, seqs: Vec<String>) -> Self {
        self.stop_sequences = Some(seqs);
        self
    }

    /// Set request metadata (currently `user_id` only).
    #[must_use]
    pub fn metadata(mut self, m: MessageMetadata) -> Self {
        self.metadata = Some(m);
        self
    }

    /// Set the request-side service tier.
    #[must_use]
    pub fn service_tier(mut self, tier: RequestServiceTier) -> Self {
        self.service_tier = Some(tier);
        self
    }

    /// Set the extended-thinking config.
    #[must_use]
    pub fn thinking(mut self, t: ThinkingConfig) -> Self {
        self.thinking = Some(t);
        self
    }

    /// Set the MCP servers exposed on this request.
    #[must_use]
    pub fn mcp_servers(mut self, servers: Vec<McpServerConfig>) -> Self {
        self.mcp_servers = servers;
        self
    }

    /// Set the container ID for the code-execution built-in tool.
    #[must_use]
    pub fn container(mut self, id: impl Into<String>) -> Self {
        self.container = Some(id.into());
        self
    }

    /// Sugar: apply an ephemeral cache breakpoint at the end of the system prompt.
    ///
    /// - `Some(Text(s))` becomes a single text block with `cache_control: ephemeral`.
    /// - `Some(Blocks(_))` has `cache_control: ephemeral` set on the last text block.
    /// - `None` is a no-op.
    #[must_use]
    pub fn cache_control_on_system(self) -> Self {
        self.cache_system_inner(CacheControl::ephemeral())
    }

    /// Shorter alias for [`Self::cache_control_on_system`].
    #[must_use]
    pub fn cache_system(self) -> Self {
        self.cache_control_on_system()
    }

    /// Like [`Self::cache_system`] but with an explicit TTL (`"5m"`,
    /// `"1h"`). The `"1h"` form requires the
    /// `extended-cache-ttl-2025-04-11` beta header.
    #[must_use]
    pub fn cache_system_with_ttl(self, ttl: impl Into<String>) -> Self {
        self.cache_system_inner(CacheControl::ephemeral_ttl(ttl))
    }

    fn cache_system_inner(mut self, cc: CacheControl) -> Self {
        let blocks = match self.system.take() {
            Some(SystemPrompt::Text(text)) => vec![ContentBlock::Known(KnownBlock::Text {
                text,
                cache_control: Some(cc),
                citations: None,
            })],
            Some(SystemPrompt::Blocks(mut blocks)) => {
                if let Some(ContentBlock::Known(KnownBlock::Text { cache_control, .. })) =
                    blocks.last_mut()
                {
                    *cache_control = Some(cc);
                }
                blocks
            }
            None => return self,
        };
        self.system = Some(SystemPrompt::Blocks(blocks));
        self
    }

    /// Sugar: apply an ephemeral cache breakpoint to the last user-authored
    /// message in the history.
    ///
    /// String content is converted to a single text block carrying
    /// `cache_control: ephemeral`. Block content has `cache_control` set on
    /// the last block that supports it (text, image, document, `tool_result`).
    /// No-op if there are no user-authored messages.
    #[must_use]
    pub fn cache_control_on_last_user(self) -> Self {
        self.cache_last_user_inner(CacheControl::ephemeral())
    }

    /// Shorter alias for [`Self::cache_control_on_last_user`].
    #[must_use]
    pub fn cache_last_user(self) -> Self {
        self.cache_control_on_last_user()
    }

    /// Like [`Self::cache_last_user`] but with an explicit TTL.
    #[must_use]
    pub fn cache_last_user_with_ttl(self, ttl: impl Into<String>) -> Self {
        self.cache_last_user_inner(CacheControl::ephemeral_ttl(ttl))
    }

    fn cache_last_user_inner(mut self, cc: CacheControl) -> Self {
        use crate::messages::input::MessageContent;
        use crate::types::Role;

        let Some(idx) = self.messages.iter().rposition(|m| m.role == Role::User) else {
            return self;
        };
        let target = &mut self.messages[idx];
        match &mut target.content {
            MessageContent::Text(text) => {
                target.content =
                    MessageContent::Blocks(vec![ContentBlock::Known(KnownBlock::Text {
                        text: std::mem::take(text),
                        cache_control: Some(cc),
                        citations: None,
                    })]);
            }
            MessageContent::Blocks(blocks) => {
                apply_cache_control_to_last_block_with(blocks, cc);
            }
        }
        self
    }

    /// Sugar: apply an ephemeral cache breakpoint to the last tool
    /// definition. The server caches all tool definitions up to that point;
    /// useful when the same tool list is reused across many requests.
    /// No-op if no tools are configured.
    #[must_use]
    pub fn cache_control_on_tools(self) -> Self {
        self.cache_tools_inner(CacheControl::ephemeral())
    }

    /// Shorter alias for [`Self::cache_control_on_tools`].
    #[must_use]
    pub fn cache_tools(self) -> Self {
        self.cache_control_on_tools()
    }

    /// Like [`Self::cache_tools`] but with an explicit TTL.
    #[must_use]
    pub fn cache_tools_with_ttl(self, ttl: impl Into<String>) -> Self {
        self.cache_tools_inner(CacheControl::ephemeral_ttl(ttl))
    }

    fn cache_tools_inner(mut self, cc: CacheControl) -> Self {
        use crate::messages::tools::Tool as MessagesTool;
        let Some(last) = self.tools.last_mut() else {
            return self;
        };
        if let MessagesTool::Custom(ct) = last {
            ct.cache_control = Some(cc);
        }
        self
    }

    /// Finalize the request.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] if `model` or `max_tokens` was not set.
    pub fn build(self) -> Result<CreateMessageRequest> {
        let model = self
            .model
            .ok_or_else(|| Error::InvalidConfig("model is required".into()))?;
        let max_tokens = self
            .max_tokens
            .ok_or_else(|| Error::InvalidConfig("max_tokens is required".into()))?;

        Ok(CreateMessageRequest {
            model,
            max_tokens,
            messages: self.messages,
            system: self.system,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            stop_sequences: self.stop_sequences,
            tools: self.tools,
            tool_choice: self.tool_choice,
            metadata: self.metadata,
            service_tier: self.service_tier,
            thinking: self.thinking,
            mcp_servers: self.mcp_servers,
            container: self.container,
            stream: false,
        })
    }
}

/// Request payload for `POST /v1/messages/count_tokens`.
///
/// Construct via [`CountTokensRequest::builder`].
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CountTokensRequest {
    /// Model whose tokenizer to use.
    pub model: ModelId,
    /// Conversation history.
    pub messages: Vec<MessageInput>,

    /// Optional system prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    /// Tools that would be exposed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    /// Tool-use policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Extended-thinking configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// MCP servers exposed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServerConfig>,
}

impl CountTokensRequest {
    /// Begin configuring a token-count request.
    #[must_use]
    pub fn builder() -> CountTokensRequestBuilder {
        CountTokensRequestBuilder::default()
    }
}

impl From<&CreateMessageRequest> for CountTokensRequest {
    /// Project a [`CreateMessageRequest`] onto the subset of fields the
    /// count-tokens endpoint accepts. Sampling parameters (`temperature`,
    /// `top_p`, etc.) and `max_tokens` are dropped because they don't
    /// affect tokenization.
    fn from(req: &CreateMessageRequest) -> Self {
        Self {
            model: req.model.clone(),
            messages: req.messages.clone(),
            system: req.system.clone(),
            tools: req.tools.clone(),
            tool_choice: req.tool_choice.clone(),
            thinking: req.thinking,
            mcp_servers: req.mcp_servers.clone(),
        }
    }
}

/// Builder for [`CountTokensRequest`].
#[derive(Debug, Default)]
pub struct CountTokensRequestBuilder {
    model: Option<ModelId>,
    messages: Vec<MessageInput>,
    system: Option<SystemPrompt>,
    tools: Vec<Tool>,
    tool_choice: Option<ToolChoice>,
    thinking: Option<ThinkingConfig>,
    mcp_servers: Vec<McpServerConfig>,
}

impl CountTokensRequestBuilder {
    /// Set the model. Required.
    #[must_use]
    pub fn model(mut self, m: impl Into<ModelId>) -> Self {
        self.model = Some(m.into());
        self
    }

    /// Replace the conversation history.
    #[must_use]
    pub fn messages(mut self, msgs: Vec<MessageInput>) -> Self {
        self.messages = msgs;
        self
    }

    /// Append a user-authored message.
    #[must_use]
    pub fn user(mut self, content: impl Into<crate::messages::input::MessageContent>) -> Self {
        self.messages.push(MessageInput::user(content));
        self
    }

    /// Append an assistant-authored message.
    #[must_use]
    pub fn assistant(mut self, content: impl Into<crate::messages::input::MessageContent>) -> Self {
        self.messages.push(MessageInput::assistant(content));
        self
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system(mut self, s: impl Into<SystemPrompt>) -> Self {
        self.system = Some(s.into());
        self
    }

    /// Set the available tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the tool-use policy.
    #[must_use]
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set the extended-thinking config.
    #[must_use]
    pub fn thinking(mut self, t: ThinkingConfig) -> Self {
        self.thinking = Some(t);
        self
    }

    /// Set the MCP servers exposed.
    #[must_use]
    pub fn mcp_servers(mut self, servers: Vec<McpServerConfig>) -> Self {
        self.mcp_servers = servers;
        self
    }

    /// Finalize the request.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] if `model` was not set.
    pub fn build(self) -> Result<CountTokensRequest> {
        let model = self
            .model
            .ok_or_else(|| Error::InvalidConfig("model is required".into()))?;
        Ok(CountTokensRequest {
            model,
            messages: self.messages,
            system: self.system,
            tools: self.tools,
            tool_choice: self.tool_choice,
            thinking: self.thinking,
            mcp_servers: self.mcp_servers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn build_requires_model_and_max_tokens() {
        let err = CreateMessageRequest::builder().build().unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));

        let err = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
    }

    #[test]
    fn minimal_request_serializes_cleanly() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(64)
            .user("hello")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(
            v,
            json!({
                "model": "claude-sonnet-4-6",
                "max_tokens": 64,
                "messages": [{"role": "user", "content": "hello"}]
            })
        );
    }

    #[test]
    fn full_request_serializes_all_fields() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::OPUS_4_7)
            .max_tokens(1024)
            .system("be concise")
            .user("hi")
            .assistant("hey, what's up")
            .user("tell me a joke")
            .temperature(0.5)
            .top_p(0.75)
            .top_k(40)
            .stop_sequences(vec!["\n\n".into()])
            .metadata(MessageMetadata::with_user("user_42"))
            .service_tier(RequestServiceTier::Auto)
            .thinking(ThinkingConfig::enabled(2048))
            .container("cnt_x")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "claude-opus-4-7");
        assert_eq!(v["max_tokens"], 1024);
        assert_eq!(v["system"], "be concise");
        assert_eq!(v["temperature"], 0.5);
        assert_eq!(v["top_p"], 0.75);
        assert_eq!(v["top_k"], 40);
        assert_eq!(v["stop_sequences"], json!(["\n\n"]));
        assert_eq!(v["metadata"]["user_id"], "user_42");
        assert_eq!(v["service_tier"], "auto");
        assert_eq!(v["thinking"]["type"], "enabled");
        assert_eq!(v["thinking"]["budget_tokens"], 2048);
        assert_eq!(v["container"], "cnt_x");
        assert_eq!(v["messages"].as_array().unwrap().len(), 3);
        // `stream` is false by default and must be omitted from the wire payload.
        assert!(
            v.get("stream").is_none(),
            "stream must be omitted when false"
        );
    }

    #[test]
    fn cache_control_on_system_converts_text_to_blocks_with_breakpoint() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .system("you are concise")
            .cache_control_on_system()
            .user("hi")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(
            v["system"],
            json!([{
                "type": "text",
                "text": "you are concise",
                "cache_control": {"type": "ephemeral"}
            }])
        );
    }

    #[test]
    fn cache_control_on_system_marks_last_text_block_when_blocks_supplied() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .system(vec![
                ContentBlock::text("first"),
                ContentBlock::text("second"),
            ])
            .cache_control_on_system()
            .user("hi")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        let blocks = v["system"].as_array().unwrap();
        assert!(blocks[0].get("cache_control").is_none());
        assert_eq!(blocks[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn cache_control_on_system_is_noop_when_no_system_set() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .cache_control_on_system()
            .user("hi")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("system").is_none());
    }

    #[test]
    fn count_tokens_minimal_request_serializes_cleanly() {
        let req = CountTokensRequest::builder()
            .model(ModelId::HAIKU_4_5)
            .user("hi")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(
            v,
            json!({
                "model": "claude-haiku-4-5-20251001",
                "messages": [{"role": "user", "content": "hi"}]
            })
        );
    }

    #[test]
    fn count_tokens_requires_model() {
        let err = CountTokensRequest::builder().build().unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
    }

    #[test]
    fn cache_control_on_last_user_converts_text_to_blocks() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("first")
            .assistant("response")
            .user("follow-up")
            .cache_control_on_last_user()
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        // First user untouched.
        assert_eq!(msgs[0]["content"], "first");
        // Last user converted to a single cached text block.
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"][0]["type"], "text");
        assert_eq!(msgs[2]["content"][0]["text"], "follow-up");
        assert_eq!(msgs[2]["content"][0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn cache_control_on_last_user_marks_last_block_when_blocks_supplied() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user(vec![ContentBlock::text("a"), ContentBlock::text("b")])
            .cache_control_on_last_user()
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        let blocks = v["messages"][0]["content"].as_array().unwrap();
        assert!(blocks[0].get("cache_control").is_none());
        assert_eq!(blocks[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn cache_control_on_last_user_is_noop_without_user_messages() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .assistant("orphan prefill")
            .cache_control_on_last_user()
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        // No user message exists; the assistant prefill is left untouched.
        assert_eq!(v["messages"][0]["content"], "orphan prefill");
    }

    #[test]
    fn cache_control_on_tools_marks_last_tool() {
        use crate::messages::tools::{CustomTool, Tool as MessagesTool};
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("hi")
            .tools(vec![
                MessagesTool::Custom(CustomTool::new("first", json!({"type": "object"}))),
                MessagesTool::Custom(CustomTool::new("second", json!({"type": "object"}))),
            ])
            .cache_control_on_tools()
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        let tools = v["tools"].as_array().unwrap();
        assert!(tools[0].get("cache_control").is_none());
        assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn cache_control_on_tools_is_noop_without_tools() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("hi")
            .cache_control_on_tools()
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("tools").is_none() || v["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn cache_system_alias_matches_long_form() {
        let short = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .system("S")
            .user("u")
            .cache_system()
            .build()
            .unwrap();
        let long = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .system("S")
            .user("u")
            .cache_control_on_system()
            .build()
            .unwrap();
        assert_eq!(
            serde_json::to_value(&short).unwrap(),
            serde_json::to_value(&long).unwrap(),
        );
    }

    #[test]
    fn cache_system_with_ttl_emits_ttl_field() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .system("S")
            .user("u")
            .cache_system_with_ttl("1h")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        let blocks = v["system"].as_array().unwrap();
        let cc = &blocks[0]["cache_control"];
        assert_eq!(cc["type"], "ephemeral");
        assert_eq!(cc["ttl"], "1h");
    }

    #[test]
    fn cache_last_user_with_ttl_emits_ttl_field() {
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("question")
            .cache_last_user_with_ttl("5m")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        let blocks = v["messages"][0]["content"].as_array().unwrap();
        let cc = &blocks[0]["cache_control"];
        assert_eq!(cc["type"], "ephemeral");
        assert_eq!(cc["ttl"], "5m");
    }

    #[test]
    fn cache_tools_with_ttl_emits_ttl_field() {
        use crate::messages::tools::CustomTool;
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("u")
            .tools(vec![Tool::Custom(CustomTool {
                name: "t".into(),
                description: None,
                input_schema: serde_json::json!({"type":"object"}),
                cache_control: None,
            })])
            .cache_tools_with_ttl("1h")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        let cc = &v["tools"][0]["cache_control"];
        assert_eq!(cc["type"], "ephemeral");
        assert_eq!(cc["ttl"], "1h");
    }
}
