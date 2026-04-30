//! Multi-turn conversation helper.
//!
//! [`Conversation`] holds the system prompt, message history, default
//! request settings, and accumulated usage for a multi-turn exchange. Each
//! call to [`Conversation::send`] runs one turn against the API and
//! appends the assistant response to the history automatically.
//!
//! Optional auto-cache mode (set via [`Conversation::with_auto_cache`] or
//! [`Conversation::with_cache_breakpoint_on_system`]) applies an ephemeral
//! `cache_control` breakpoint to the system prompt and optionally the most
//! recent user turn before each request, so cache hits stay high without
//! the app needing to think about it.
//!
//! [`Conversation`] is `Serialize + Deserialize`, so a session can be
//! persisted to disk and resumed later.
//!
//! Gated on the `conversation` feature.

use serde::{Deserialize, Serialize};

use crate::messages::cache::CacheControl;
use crate::messages::content::{ContentBlock, KnownBlock};
use crate::messages::input::{MessageContent, MessageInput, SystemPrompt};
use crate::messages::mcp::McpServerConfig;
use crate::messages::metadata::{MessageMetadata, RequestServiceTier};
use crate::messages::request::CreateMessageRequest;
use crate::messages::thinking::ThinkingConfig;
use crate::messages::tools::{Tool, ToolChoice};
use crate::types::{ModelId, Role, Usage};

#[cfg(feature = "async")]
use crate::client::Client;
#[cfg(feature = "async")]
use crate::error::Result;
#[cfg(feature = "async")]
use crate::messages::response::Message;

/// Multi-turn conversation state plus per-request defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Conversation {
    /// Model used for new turns (also recorded with each `UsageRecord`).
    pub model: ModelId,
    /// Maximum output tokens per turn.
    pub max_tokens: u32,

    /// Optional system prompt; survives across turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,

    /// Conversation history, oldest first.
    #[serde(default)]
    pub messages: Vec<MessageInput>,

    /// Default sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Default nucleus sampling cutoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Default top-k cutoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Default stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Tools made available to every turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    /// Default tool-use policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Default extended-thinking config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// Default request metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
    /// Default request-side service tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<RequestServiceTier>,
    /// MCP servers exposed on every turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServerConfig>,
    /// Container ID for the code-execution built-in tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,

    /// Auto-cache configuration applied at request-build time.
    #[serde(default)]
    pub auto_cache: AutoCacheMode,

    /// Per-turn `Usage` records, oldest first. Updated by [`Self::send`].
    #[serde(default)]
    pub usage_history: Vec<UsageRecord>,
}

/// One turn's `Usage` paired with the model it ran on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UsageRecord {
    /// Model that produced this usage record.
    pub model: ModelId,
    /// Usage as reported by the API.
    pub usage: Usage,
}

/// Automatic cache-breakpoint placement for outgoing requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AutoCacheMode {
    /// No automatic cache breakpoints. Default.
    #[default]
    Off,
    /// Apply ephemeral `cache_control` to the last block of the system prompt.
    System,
    /// Apply ephemeral `cache_control` to the system prompt's last block AND
    /// to the most recent user turn's last block.
    SystemAndLastUser,
}

impl Conversation {
    /// Begin a new conversation with the given model and per-turn `max_tokens`.
    #[must_use]
    pub fn new(model: impl Into<ModelId>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            system: None,
            messages: Vec::new(),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: Vec::new(),
            tool_choice: None,
            thinking: None,
            metadata: None,
            service_tier: None,
            mcp_servers: Vec::new(),
            container: None,
            auto_cache: AutoCacheMode::Off,
            usage_history: Vec::new(),
        }
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system(mut self, s: impl Into<SystemPrompt>) -> Self {
        self.system = Some(s.into());
        self
    }

    /// Shorthand for setting [`AutoCacheMode::System`] via
    /// [`Self::with_auto_cache`].
    #[must_use]
    pub fn with_cache_breakpoint_on_system(self) -> Self {
        self.with_auto_cache(AutoCacheMode::System)
    }

    /// Set the auto-cache mode. See [`AutoCacheMode`].
    #[must_use]
    pub fn with_auto_cache(mut self, mode: AutoCacheMode) -> Self {
        self.auto_cache = mode;
        self
    }

    /// Replace the tool list.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the tool-use policy.
    #[must_use]
    pub fn with_tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Enable extended thinking.
    #[must_use]
    pub fn with_thinking(mut self, t: ThinkingConfig) -> Self {
        self.thinking = Some(t);
        self
    }

    /// Set the sampling temperature default.
    #[must_use]
    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Append a user-authored turn.
    pub fn push_user(&mut self, content: impl Into<MessageContent>) {
        self.messages.push(MessageInput::user(content));
    }

    /// Append an assistant-authored turn (typically used for prefill before
    /// the first send).
    pub fn push_assistant(&mut self, content: impl Into<MessageContent>) {
        self.messages.push(MessageInput::assistant(content));
    }

    /// Remove and return the most recent message. Useful when aborting a
    /// turn before sending.
    pub fn pop(&mut self) -> Option<MessageInput> {
        self.messages.pop()
    }

    /// Number of completed turns (request/response cycles via [`Self::send`]).
    #[must_use]
    pub fn turn_count(&self) -> usize {
        self.usage_history.len()
    }

    /// Sum of every recorded `Usage` for this conversation.
    #[must_use]
    pub fn cumulative_usage(&self) -> Usage {
        self.usage_history
            .iter()
            .fold(Usage::default(), |mut acc, r| {
                acc.input_tokens = acc.input_tokens.saturating_add(r.usage.input_tokens);
                acc.output_tokens = acc.output_tokens.saturating_add(r.usage.output_tokens);
                acc.cache_creation_input_tokens = sum_opt(
                    acc.cache_creation_input_tokens,
                    r.usage.cache_creation_input_tokens,
                );
                acc.cache_read_input_tokens =
                    sum_opt(acc.cache_read_input_tokens, r.usage.cache_read_input_tokens);
                acc
            })
    }

    /// Total cost in USD across all recorded turns, using the given pricing
    /// table to look up rates for each turn's model.
    #[cfg(feature = "pricing")]
    #[cfg_attr(docsrs, doc(cfg(feature = "pricing")))]
    #[must_use]
    pub fn cost(&self, pricing: &crate::pricing::PricingTable) -> f64 {
        self.usage_history
            .iter()
            .map(|r| pricing.cost(&r.model, &r.usage))
            .sum()
    }

    /// Build the [`CreateMessageRequest`] this conversation would send next,
    /// including any auto-cache breakpoints. Pure -- does not touch state.
    ///
    /// # Panics
    ///
    /// Will not panic in practice: the conversation always carries `model`
    /// and `max_tokens`, so the inner builder's `build()` always succeeds.
    #[must_use]
    pub fn build_request(&self) -> CreateMessageRequest {
        let mut messages = self.messages.clone();
        let mut system = self.system.clone();

        match self.auto_cache {
            AutoCacheMode::Off => {}
            AutoCacheMode::System => {
                cache_breakpoint_on_system(&mut system);
            }
            AutoCacheMode::SystemAndLastUser => {
                cache_breakpoint_on_system(&mut system);
                cache_breakpoint_on_last_user(&mut messages);
            }
        }

        let mut builder = CreateMessageRequest::builder()
            .model(self.model.clone())
            .max_tokens(self.max_tokens)
            .messages(messages);

        if let Some(s) = system {
            builder = builder.system(s);
        }
        if let Some(t) = self.temperature {
            builder = builder.temperature(t);
        }
        if let Some(p) = self.top_p {
            builder = builder.top_p(p);
        }
        if let Some(k) = self.top_k {
            builder = builder.top_k(k);
        }
        if let Some(seqs) = &self.stop_sequences {
            builder = builder.stop_sequences(seqs.clone());
        }
        if !self.tools.is_empty() {
            builder = builder.tools(self.tools.clone());
        }
        if let Some(c) = self.tool_choice.clone() {
            builder = builder.tool_choice(c);
        }
        if let Some(t) = self.thinking {
            builder = builder.thinking(t);
        }
        if let Some(m) = self.metadata.clone() {
            builder = builder.metadata(m);
        }
        if let Some(t) = self.service_tier {
            builder = builder.service_tier(t);
        }
        if !self.mcp_servers.is_empty() {
            builder = builder.mcp_servers(self.mcp_servers.clone());
        }
        if let Some(c) = self.container.clone() {
            builder = builder.container(c);
        }

        builder
            .build()
            .expect("conversation::build_request always provides model + max_tokens")
    }

    /// Drive one turn against the API. Appends the assistant response to
    /// the history and records the usage.
    #[cfg(feature = "async")]
    #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
    pub async fn send(&mut self, client: &Client) -> Result<Message> {
        self.send_with_beta(client, &[]).await
    }

    /// Like [`Self::send`] but with per-request beta headers merged in.
    #[cfg(feature = "async")]
    #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
    pub async fn send_with_beta(&mut self, client: &Client, betas: &[&str]) -> Result<Message> {
        let request = self.build_request();
        let response = client.messages().create_with_beta(request, betas).await?;
        self.usage_history.push(UsageRecord {
            model: self.model.clone(),
            usage: response.usage.clone(),
        });
        // Append the assistant turn so subsequent sends see it.
        self.messages
            .push(MessageInput::assistant(response.content.clone()));
        Ok(response)
    }
}

fn sum_opt(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (None, None) => None,
        (Some(x), None) | (None, Some(x)) => Some(x),
        (Some(x), Some(y)) => Some(x.saturating_add(y)),
    }
}

fn cache_breakpoint_on_system(system: &mut Option<SystemPrompt>) {
    let Some(s) = system.take() else { return };
    let blocks = match s {
        SystemPrompt::Text(text) => vec![ContentBlock::Known(KnownBlock::Text {
            text,
            cache_control: Some(CacheControl::ephemeral()),
            citations: None,
        })],
        SystemPrompt::Blocks(mut blocks) => {
            apply_cache_control_to_last_block(&mut blocks);
            blocks
        }
    };
    *system = Some(SystemPrompt::Blocks(blocks));
}

fn cache_breakpoint_on_last_user(messages: &mut [MessageInput]) {
    let Some(idx) = messages.iter().rposition(|m| m.role == Role::User) else {
        return;
    };
    let target = &mut messages[idx];
    match &mut target.content {
        MessageContent::Text(text) => {
            target.content = MessageContent::Blocks(vec![ContentBlock::Known(KnownBlock::Text {
                text: std::mem::take(text),
                cache_control: Some(CacheControl::ephemeral()),
                citations: None,
            })]);
        }
        MessageContent::Blocks(blocks) => {
            apply_cache_control_to_last_block(blocks);
        }
    }
}

fn apply_cache_control_to_last_block(blocks: &mut [ContentBlock]) {
    let Some(last) = blocks.last_mut() else {
        return;
    };
    // Collapsed `if let ... { match ... }` into a single nested pattern.
    // Variants without a `cache_control` field (ToolUse, Thinking,
    // RedactedThinking, ServerToolUse, WebSearchToolResult) and
    // `ContentBlock::Other` simply don't match -- the cache hint is silently
    // skipped, which is the right behavior for an auto-cache helper.
    if let ContentBlock::Known(
        KnownBlock::Text { cache_control, .. }
        | KnownBlock::Image { cache_control, .. }
        | KnownBlock::Document { cache_control, .. }
        | KnownBlock::ToolResult { cache_control, .. },
    ) = last
    {
        *cache_control = Some(CacheControl::ephemeral());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn convo() -> Conversation {
        Conversation::new(ModelId::SONNET_4_6, 256)
    }

    // ---- basic state + serde -----------------------------------------------

    #[test]
    fn new_starts_empty() {
        let c = convo();
        assert!(c.messages.is_empty());
        assert!(c.usage_history.is_empty());
        assert_eq!(c.turn_count(), 0);
    }

    #[test]
    fn push_appends_to_history() {
        let mut c = convo();
        c.push_user("hi");
        c.push_assistant("hello");
        c.push_user("how are you?");
        assert_eq!(c.messages.len(), 3);
        assert_eq!(c.messages[0].role, Role::User);
        assert_eq!(c.messages[1].role, Role::Assistant);
    }

    #[test]
    fn pop_removes_last() {
        let mut c = convo();
        c.push_user("first");
        c.push_user("second");
        let popped = c.pop().unwrap();
        let MessageContent::Text(t) = popped.content else {
            panic!("expected Text content");
        };
        assert_eq!(t, "second");
        assert_eq!(c.messages.len(), 1);
    }

    #[test]
    fn cumulative_usage_sums_across_turns() {
        let mut c = convo();
        c.usage_history.push(UsageRecord {
            model: ModelId::SONNET_4_6,
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_input_tokens: Some(20),
                cache_read_input_tokens: Some(30),
                ..Usage::default()
            },
        });
        c.usage_history.push(UsageRecord {
            model: ModelId::SONNET_4_6,
            usage: Usage {
                input_tokens: 200,
                output_tokens: 80,
                cache_read_input_tokens: Some(70),
                ..Usage::default()
            },
        });
        let total = c.cumulative_usage();
        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 130);
        assert_eq!(total.cache_creation_input_tokens, Some(20));
        assert_eq!(total.cache_read_input_tokens, Some(100));
    }

    #[test]
    fn serde_round_trip_preserves_state() {
        let mut original = Conversation::new(ModelId::OPUS_4_7, 512)
            .system("be concise")
            .with_cache_breakpoint_on_system()
            .with_temperature(0.5);
        original.push_user("hi");
        original.push_assistant("hello");
        original.usage_history.push(UsageRecord {
            model: ModelId::OPUS_4_7,
            usage: Usage {
                input_tokens: 5,
                output_tokens: 3,
                ..Usage::default()
            },
        });

        let json = serde_json::to_string(&original).unwrap();
        let parsed: Conversation = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.model, ModelId::OPUS_4_7);
        assert_eq!(parsed.max_tokens, 512);
        assert_eq!(parsed.auto_cache, AutoCacheMode::System);
        assert_eq!(parsed.temperature, Some(0.5));
        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.usage_history.len(), 1);
        assert_eq!(parsed.turn_count(), 1);
    }

    // ---- request building --------------------------------------------------

    #[test]
    fn build_request_includes_basic_fields() {
        let mut c = convo().system("be concise").with_temperature(0.25);
        c.push_user("hello");
        let req = c.build_request();
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "claude-sonnet-4-6");
        assert_eq!(v["max_tokens"], 256);
        assert_eq!(v["system"], "be concise");
        assert_eq!(v["temperature"], 0.25);
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn build_request_with_auto_cache_system() {
        let mut c = convo()
            .system("you are concise")
            .with_cache_breakpoint_on_system();
        c.push_user("hi");
        let v = serde_json::to_value(c.build_request()).unwrap();
        assert_eq!(
            v["system"],
            json!([{
                "type": "text",
                "text": "you are concise",
                "cache_control": {"type": "ephemeral"}
            }])
        );
        // Last user message should NOT be cached in this mode.
        assert_eq!(v["messages"][0]["content"], "hi");
    }

    #[test]
    fn build_request_with_auto_cache_system_and_last_user() {
        let mut c = convo()
            .system("you are concise")
            .with_auto_cache(AutoCacheMode::SystemAndLastUser);
        c.push_user("first");
        c.push_assistant("response");
        c.push_user("follow-up");
        let v = serde_json::to_value(c.build_request()).unwrap();

        // System cached
        assert_eq!(v["system"][0]["cache_control"]["type"], "ephemeral");

        // Last user (index 2) cached as a single text block
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"][0]["type"], "text");
        assert_eq!(msgs[2]["content"][0]["text"], "follow-up");
        assert_eq!(msgs[2]["content"][0]["cache_control"]["type"], "ephemeral");

        // Earlier user message (index 0) untouched.
        assert_eq!(msgs[0]["content"], "first");
    }

    #[test]
    fn build_request_auto_cache_off_does_nothing() {
        let mut c = convo().system("plain");
        c.push_user("hi");
        let v = serde_json::to_value(c.build_request()).unwrap();
        // System remains a plain string.
        assert_eq!(v["system"], "plain");
        // User message remains a plain string.
        assert_eq!(v["messages"][0]["content"], "hi");
    }

    #[test]
    fn build_request_does_not_mutate_self() {
        let mut c = convo().system("orig").with_cache_breakpoint_on_system();
        c.push_user("hi");
        let _ = c.build_request();
        // After build, the conversation's stored system is still the plain
        // text -- auto-cache is applied at request-build time, not stored.
        let Some(SystemPrompt::Text(t)) = &c.system else {
            panic!("system should still be Text, got {:?}", c.system);
        };
        assert_eq!(t, "orig");
        let MessageContent::Text(t) = &c.messages[0].content else {
            panic!(
                "user content should still be Text, got {:?}",
                c.messages[0].content
            );
        };
        assert_eq!(t, "hi");
    }

    // ---- pricing integration -----------------------------------------------

    #[cfg(feature = "pricing")]
    #[test]
    fn cost_uses_pricing_table_per_turn_model() {
        let pricing = crate::pricing::PricingTable::default();
        let mut c = convo();
        c.usage_history.push(UsageRecord {
            model: ModelId::SONNET_4_6,
            usage: Usage {
                input_tokens: 1_000_000,
                ..Usage::default()
            },
        });
        c.usage_history.push(UsageRecord {
            model: ModelId::HAIKU_4_5,
            usage: Usage {
                input_tokens: 1_000_000,
                ..Usage::default()
            },
        });
        // Sonnet 4.6 = $3/MTok input, Haiku 4.5 = $1/MTok input -> $4.0
        let total = c.cost(&pricing);
        assert!((total - 4.0).abs() < 1e-9, "expected $4.00, got ${total}");
    }
}

#[cfg(all(test, feature = "async"))]
mod api_tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn fake_response(text: &str, input: u32, output: u32) -> serde_json::Value {
        json!({
            "id": "msg_x",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": text}],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": input, "output_tokens": output}
        })
    }

    #[tokio::test]
    async fn send_appends_assistant_turn_and_records_usage() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response("hi back", 5, 2)))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut c = Conversation::new(ModelId::SONNET_4_6, 64);
        c.push_user("hi");

        let r = c.send(&client).await.unwrap();
        assert_eq!(r.id, "msg_x");

        // History now has user + assistant.
        assert_eq!(c.messages.len(), 2);
        assert_eq!(c.messages[1].role, Role::Assistant);

        // Usage was recorded with the conversation's model.
        assert_eq!(c.turn_count(), 1);
        assert_eq!(c.usage_history[0].model, ModelId::SONNET_4_6);
        assert_eq!(c.usage_history[0].usage.input_tokens, 5);
        assert_eq!(c.usage_history[0].usage.output_tokens, 2);
    }

    #[tokio::test]
    async fn second_send_includes_first_assistant_turn_in_history() {
        let mock = MockServer::start().await;
        // First call -- any user prompt OK.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response("first", 5, 3)))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        // Second call must contain the first assistant turn AND the new user turn.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "content": [{"type": "text", "text": "first"}]},
                    {"role": "user", "content": "again"}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response("second", 8, 4)))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut c = Conversation::new(ModelId::SONNET_4_6, 64);
        c.push_user("hi");
        let _ = c.send(&client).await.unwrap();
        c.push_user("again");
        let _ = c.send(&client).await.unwrap();

        assert_eq!(c.turn_count(), 2);
        let total = c.cumulative_usage();
        assert_eq!(total.input_tokens, 13);
        assert_eq!(total.output_tokens, 7);
    }

    #[tokio::test]
    async fn auto_cache_system_sends_cache_control_in_request_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({
                "system": [{
                    "type": "text",
                    "text": "be concise",
                    "cache_control": {"type": "ephemeral"}
                }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response("ok", 3, 1)))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut c = Conversation::new(ModelId::SONNET_4_6, 32)
            .system("be concise")
            .with_cache_breakpoint_on_system();
        c.push_user("hello");
        let _ = c.send(&client).await.unwrap();
    }
}
