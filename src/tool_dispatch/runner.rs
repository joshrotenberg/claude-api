//! The agent loop runner.
//!
//! [`Client::run`] drives the `tool_use -> tool_result` loop until the
//! model stops requesting tools (or the iteration cap is hit). Collapses
//! the manual loop in `examples/tool_use.rs` into a single call.
//!
//! Gated on the `conversation` feature in addition to the parent
//! `tool_dispatch` module's `async` gate.

#![cfg(feature = "conversation")]

use std::fmt;

use crate::client::Client;
use crate::conversation::{Conversation, UsageRecord};
use crate::error::{Error, Result};
use crate::messages::content::{ContentBlock, KnownBlock, ToolResultContent};
use crate::messages::input::MessageInput;
use crate::messages::response::Message;
use crate::tool_dispatch::registry::ToolRegistry;
use crate::types::StopReason;

/// Type alias for the per-iteration callback hook.
type IterationHook = Box<dyn Fn(&Message, u32) + Send + Sync + 'static>;

/// Optional knobs for the agent loop.
///
/// Build via [`RunOptions::default`] and chain setters; see method docs.
pub struct RunOptions {
    max_iterations: u32,
    on_iteration: Option<IterationHook>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            max_iterations: 16,
            on_iteration: None,
        }
    }
}

impl RunOptions {
    /// Equivalent to [`Self::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Maximum total iterations of the model loop. Default 16.
    #[must_use]
    pub fn max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    /// Hook invoked after each `messages.create` response. Receives the
    /// response and the 1-indexed iteration number. Useful for streaming
    /// progress to the UI or recording fine-grained traces.
    #[must_use]
    pub fn on_iteration<F>(mut self, hook: F) -> Self
    where
        F: Fn(&Message, u32) + Send + Sync + 'static,
    {
        self.on_iteration = Some(Box::new(hook));
        self
    }

    /// Borrow the configured iteration cap.
    #[must_use]
    pub fn max_iterations_value(&self) -> u32 {
        self.max_iterations
    }
}

impl fmt::Debug for RunOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunOptions")
            .field("max_iterations", &self.max_iterations)
            .field(
                "on_iteration",
                &self.on_iteration.as_ref().map(|_| "<closure>"),
            )
            .finish()
    }
}

impl Client {
    /// Drive a multi-turn agent loop against this client.
    ///
    /// Each iteration:
    ///
    /// 1. Builds a `CreateMessageRequest` from `conversation`, overriding
    ///    its `tools` field with `registry.to_messages_tools()`.
    /// 2. Sends it (retries handled by the client's configured retry policy).
    /// 3. Records the response's `Usage` on the conversation.
    /// 4. Appends the assistant's full response (text + `tool_use` blocks)
    ///    to the conversation history.
    /// 5. If `stop_reason != ToolUse`, returns the response.
    /// 6. Otherwise dispatches each `tool_use` block via `registry`,
    ///    builds matching `tool_result` blocks (with `is_error = true`
    ///    for failures), appends them as a user turn, and loops.
    ///
    /// Returns [`Error::MaxIterationsExceeded`] if the loop hits
    /// `options.max_iterations` without the model terminating. Tool
    /// execution errors do *not* propagate; they are surfaced back to
    /// the model as `is_error = true` tool results so it can recover.
    pub async fn run(
        &self,
        conversation: &mut Conversation,
        registry: &ToolRegistry,
        options: RunOptions,
    ) -> Result<Message> {
        for iteration in 1..=options.max_iterations {
            let span = tracing::info_span!("agent_iteration", iteration);
            let _enter = span.enter();

            // Apply context compaction if the conversation has a policy
            // configured. Long-running agent loops are exactly where this
            // matters most.
            conversation.compact_if_needed();

            // Build the request, replacing the conversation's tools with the
            // registry's authoritative list. Documented behavior: in run()
            // mode the registry is the source of truth for tool definitions.
            let mut request = conversation.build_request();
            request.tools = registry.to_messages_tools();

            let response = self.messages().create(request).await?;

            // Update conversation state.
            conversation.usage_history.push(UsageRecord {
                model: conversation.model.clone(),
                usage: response.usage.clone(),
            });
            conversation
                .messages
                .push(MessageInput::assistant(response.content.clone()));

            if let Some(hook) = &options.on_iteration {
                hook(&response, iteration);
            }

            if response.stop_reason != Some(StopReason::ToolUse) {
                return Ok(response);
            }

            // Collect every tool_use block, dispatch, build tool_result blocks.
            let mut tool_results: Vec<ContentBlock> = Vec::new();
            for block in &response.content {
                if let ContentBlock::Known(KnownBlock::ToolUse { id, name, input }) = block {
                    let dispatched = registry.dispatch(name, input.clone()).await;
                    let (content, is_error) = match dispatched {
                        Ok(value) => (value_to_tool_result(value), None),
                        Err(e) => {
                            tracing::warn!(
                                tool = %name,
                                error = %e,
                                "claude-api: tool dispatch error -- surfacing to model as is_error",
                            );
                            (ToolResultContent::Text(format!("{e}")), Some(true))
                        }
                    };
                    tool_results.push(ContentBlock::Known(KnownBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content,
                        is_error,
                        cache_control: None,
                    }));
                }
            }

            // Defensive: model said ToolUse but emitted no tool_use blocks.
            // Return the response and let the caller decide what to do.
            if tool_results.is_empty() {
                return Ok(response);
            }

            conversation.messages.push(MessageInput::user(tool_results));
        }

        Err(Error::MaxIterationsExceeded {
            max: options.max_iterations,
        })
    }
}

fn value_to_tool_result(value: serde_json::Value) -> ToolResultContent {
    // String results pass through cleanly; everything else gets serialized
    // back to a string (the model is comfortable with JSON-as-text).
    match value {
        serde_json::Value::String(s) => ToolResultContent::Text(s),
        other => ToolResultContent::Text(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::Conversation;
    use crate::messages::tools::Tool as MessagesTool;
    use crate::tool_dispatch::tool::ToolError;
    use crate::types::ModelId;
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn echo_registry() -> ToolRegistry {
        let mut r = ToolRegistry::new();
        r.register(
            "echo",
            json!({"type": "object", "properties": {"text": {"type": "string"}}}),
            |input| async move { Ok(input) },
        );
        r
    }

    fn assistant_text(text: &str, stop: &str) -> Value {
        json!({
            "id": "msg_t",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": text}],
            "model": "claude-sonnet-4-6",
            "stop_reason": stop,
            "usage": {"input_tokens": 5, "output_tokens": 3}
        })
    }

    #[allow(clippy::needless_pass_by_value)]
    fn assistant_tool_use(id: &str, name: &str, input: Value) -> Value {
        json!({
            "id": "msg_t",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "calling tool"},
                {"type": "tool_use", "id": id, "name": name, "input": input}
            ],
            "model": "claude-sonnet-4-6",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })
    }

    #[tokio::test]
    async fn single_turn_no_tools_returns_immediately() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("done", "end_turn")),
            )
            .expect(1)
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let registry = ToolRegistry::new();
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("hi");

        let resp = client
            .run(&mut convo, &registry, RunOptions::default())
            .await
            .unwrap();
        assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(convo.turn_count(), 1);
        // Only assistant turn appended; no tool_result user turn since no tool_use.
        assert_eq!(convo.messages.len(), 2);
    }

    #[tokio::test]
    async fn two_turn_tool_use_loop_completes() {
        let mock = MockServer::start().await;
        // Iteration 1: model asks to call echo with {"text":"hello"}
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(assistant_tool_use(
                "toolu_1",
                "echo",
                json!({"text":"hello"}),
            )))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        // Iteration 2: must include the tool_result; model finishes with end_turn.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({
                "messages": [
                    {"role": "user", "content": "say hello"},
                    {"role": "assistant", "content": [
                        {"type": "text", "text": "calling tool"},
                        {"type": "tool_use", "id": "toolu_1", "name": "echo", "input": {"text":"hello"}}
                    ]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "toolu_1", "content": "{\"text\":\"hello\"}"}
                    ]}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(assistant_text("said hello!", "end_turn")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 256);
        convo.push_user("say hello");

        let resp = client
            .run(&mut convo, &echo_registry(), RunOptions::default())
            .await
            .unwrap();

        assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
        // Turn count is 2 (one tool_use + one final response).
        assert_eq!(convo.turn_count(), 2);
        // History: initial user + assistant tool_use + user tool_result + final assistant.
        assert_eq!(convo.messages.len(), 4);
    }

    #[tokio::test]
    async fn max_iterations_returns_error_and_records_each_turn() {
        let mock = MockServer::start().await;
        // Always respond with a tool_use so the loop never terminates naturally.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(assistant_tool_use(
                "toolu_x",
                "echo",
                json!({"text":"x"}),
            )))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("loop");

        let err = client
            .run(
                &mut convo,
                &echo_registry(),
                RunOptions::default().max_iterations(3),
            )
            .await
            .unwrap_err();

        let Error::MaxIterationsExceeded { max } = err else {
            panic!("expected MaxIterationsExceeded, got {err:?}");
        };
        assert_eq!(max, 3);
        assert_eq!(convo.turn_count(), 3);
        // History: original user + 3*(assistant tool_use + user tool_result)
        assert_eq!(convo.messages.len(), 1 + 3 * 2);
    }

    #[tokio::test]
    async fn tool_error_becomes_is_error_tool_result() {
        let mock = MockServer::start().await;
        // Iteration 1: model calls a tool we'll fail intentionally.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(assistant_tool_use(
                "toolu_e",
                "boom",
                json!({}),
            )))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        // Iteration 2: must see the is_error=true tool_result.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({
                "messages": [
                    {"role": "user", "content": "fail"},
                    {"role": "assistant"},
                    {"role": "user", "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_e",
                        "is_error": true
                    }]}
                ]
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("recovered", "end_turn")),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut registry = ToolRegistry::new();
        registry.register("boom", json!({}), |_input| async move {
            Err(ToolError::execution(std::io::Error::other("kaboom")))
        });

        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("fail");
        let resp = client
            .run(&mut convo, &registry, RunOptions::default())
            .await
            .unwrap();
        assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
    }

    #[tokio::test]
    async fn unknown_tool_becomes_is_error_with_unknown_message() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(assistant_tool_use(
                "toolu_u",
                "missing",
                json!({}),
            )))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("ok", "end_turn")),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("call missing");

        let _ = client
            .run(&mut convo, &ToolRegistry::new(), RunOptions::default())
            .await
            .unwrap();

        // The user-turn that carries the tool_result should mention the unknown tool name.
        let user_turn = &convo.messages[2];
        let serialized = serde_json::to_string(&user_turn.content).unwrap();
        assert!(
            serialized.contains("no tool registered with name 'missing'"),
            "{serialized}"
        );
        assert!(serialized.contains("\"is_error\":true"));
    }

    #[tokio::test]
    async fn run_uses_registry_tools_not_conversation_tools() {
        // The conversation has its own tools list, but run() is supposed to
        // override with registry.to_messages_tools(). Verify by asserting on
        // the request body: the wire `tools` array must contain "echo".
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({
                "tools": [{"name": "echo"}]
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("ok", "end_turn")),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        // Conversation has a stale tool that we expect to be overridden.
        let mut convo =
            Conversation::new(ModelId::SONNET_4_6, 64).with_tools(vec![MessagesTool::Custom(
                crate::messages::tools::CustomTool::new("stale", json!({"type": "object"})),
            )]);
        convo.push_user("hi");

        let _ = client
            .run(&mut convo, &echo_registry(), RunOptions::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn on_iteration_callback_fires_per_iteration() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(assistant_tool_use(
                "toolu_h",
                "echo",
                json!({"text":"x"}),
            )))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("done", "end_turn")),
            )
            .mount(&mock)
            .await;

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);
        let options = RunOptions::default().on_iteration(move |_msg, n| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            // Iteration is 1-indexed and matches the call count.
            assert!(n >= 1);
        });

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("hi");

        let _ = client
            .run(&mut convo, &echo_registry(), options)
            .await
            .unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn tool_returning_string_value_passes_through_cleanly() {
        // Whitebox: verify value_to_tool_result mapping doesn't double-quote a String.
        let result = value_to_tool_result(json!("plain text"));
        let ToolResultContent::Text(t) = result else {
            panic!("expected Text");
        };
        assert_eq!(t, "plain text");
    }

    #[tokio::test]
    async fn tool_returning_object_value_serializes_to_json_string() {
        let result = value_to_tool_result(json!({"k": 42}));
        let ToolResultContent::Text(t) = result else {
            panic!("expected Text");
        };
        // Round-trip the JSON to verify shape is preserved.
        let parsed: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(parsed, json!({"k": 42}));
    }
}
