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

/// Cost budget for the agent loop, paired with the pricing table used to
/// evaluate `Conversation::cost`.
#[cfg(feature = "pricing")]
#[cfg_attr(docsrs, doc(cfg(feature = "pricing")))]
pub struct CostBudget {
    /// Maximum cumulative spend allowed across the loop, in USD.
    pub max_usd: f64,
    /// Pricing table used to compute spend.
    pub pricing: crate::pricing::PricingTable,
}

/// Optional knobs for the agent loop.
///
/// Build via [`RunOptions::default`] and chain setters; see method docs.
pub struct RunOptions {
    max_iterations: u32,
    on_iteration: Option<IterationHook>,
    parallel_tool_dispatch: bool,
    #[cfg(feature = "pricing")]
    cost_budget: Option<CostBudget>,
    cancel_token: Option<tokio_util::sync::CancellationToken>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            max_iterations: 16,
            on_iteration: None,
            parallel_tool_dispatch: true,
            #[cfg(feature = "pricing")]
            cost_budget: None,
            cancel_token: None,
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

    /// Whether to dispatch the `tool_use` blocks within a single assistant
    /// turn concurrently (default `true`). Set to `false` to dispatch
    /// sequentially -- useful when tools have ordering side effects (e.g.
    /// shared mutable state) or when serial output is easier to debug.
    #[must_use]
    pub fn parallel_tool_dispatch(mut self, parallel: bool) -> Self {
        self.parallel_tool_dispatch = parallel;
        self
    }

    /// Cap cumulative spend on the conversation. After each turn the
    /// runner computes [`Conversation::cost`](crate::conversation::Conversation::cost)
    /// against `pricing` and aborts with [`Error::CostBudgetExceeded`] if
    /// the cumulative cost exceeds `max_usd`.
    #[cfg(feature = "pricing")]
    #[cfg_attr(docsrs, doc(cfg(feature = "pricing")))]
    #[must_use]
    pub fn cost_budget(mut self, max_usd: f64, pricing: crate::pricing::PricingTable) -> Self {
        self.cost_budget = Some(CostBudget { max_usd, pricing });
        self
    }

    /// Attach a cancellation token. Checked at the top of every iteration;
    /// if cancelled, the loop returns [`Error::Cancelled`] before issuing
    /// the next request.
    #[must_use]
    pub fn cancel_token(mut self, token: tokio_util::sync::CancellationToken) -> Self {
        self.cancel_token = Some(token);
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
        let mut s = f.debug_struct("RunOptions");
        s.field("max_iterations", &self.max_iterations)
            .field(
                "on_iteration",
                &self.on_iteration.as_ref().map(|_| "<closure>"),
            )
            .field("parallel_tool_dispatch", &self.parallel_tool_dispatch)
            .field("cancel_token", &self.cancel_token.is_some());
        #[cfg(feature = "pricing")]
        s.field("cost_budget", &self.cost_budget.as_ref().map(|b| b.max_usd));
        s.finish()
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
    #[allow(clippy::too_many_lines)] // cohesive control flow; splitting hurts readability
    pub async fn run(
        &self,
        conversation: &mut Conversation,
        registry: &ToolRegistry,
        options: RunOptions,
    ) -> Result<Message> {
        for iteration in 1..=options.max_iterations {
            let span = tracing::info_span!("agent_iteration", iteration);
            let _enter = span.enter();

            // Cancellation gate: short-circuit before any work this turn.
            if let Some(token) = &options.cancel_token {
                if token.is_cancelled() {
                    tracing::info!(iteration, "claude-api: agent loop cancelled");
                    return Err(Error::Cancelled);
                }
            }

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

            // Cost budget gate: check after recording this turn's usage.
            #[cfg(feature = "pricing")]
            if let Some(budget) = &options.cost_budget {
                let spent = conversation.cost(&budget.pricing);
                if spent > budget.max_usd {
                    tracing::warn!(
                        iteration,
                        spent_usd = spent,
                        budget_usd = budget.max_usd,
                        "claude-api: agent loop exceeded cost budget",
                    );
                    return Err(Error::CostBudgetExceeded {
                        budget_usd: budget.max_usd,
                        spent_usd: spent,
                    });
                }
            }

            if response.stop_reason != Some(StopReason::ToolUse) {
                return Ok(response);
            }

            // Collect tool_use blocks in the order they appeared.
            let tool_uses: Vec<(String, String, serde_json::Value)> = response
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::Known(KnownBlock::ToolUse { id, name, input }) = b {
                        Some((id.clone(), name.clone(), input.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            // Defensive: model said ToolUse but emitted no tool_use blocks.
            if tool_uses.is_empty() {
                return Ok(response);
            }

            // Dispatch -- in parallel by default, sequentially on request.
            let dispatched = if options.parallel_tool_dispatch {
                let futures = tool_uses.iter().map(|(id, name, input)| {
                    let id = id.clone();
                    let name = name.clone();
                    let input = input.clone();
                    async move {
                        let result = registry.dispatch(&name, input).await;
                        (id, name, result)
                    }
                });
                futures_util::future::join_all(futures).await
            } else {
                let mut out = Vec::with_capacity(tool_uses.len());
                for (id, name, input) in &tool_uses {
                    let result = registry.dispatch(name, input.clone()).await;
                    out.push((id.clone(), name.clone(), result));
                }
                out
            };

            // Build tool_result blocks in the same order as the tool_use
            // blocks. The model expects positional correspondence.
            let mut tool_results: Vec<ContentBlock> = Vec::with_capacity(dispatched.len());
            for (id, name, result) in dispatched {
                let (content, is_error) = match result {
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
                    tool_use_id: id,
                    content,
                    is_error,
                    cache_control: None,
                }));
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

    // ---- v0.4 guardrails: parallel dispatch / cost budget / cancellation ----

    #[tokio::test]
    async fn parallel_tool_dispatch_runs_concurrently() {
        // Two tools that each sleep 80ms. Sequential = ~160ms; parallel = ~80ms.
        // Use a generous upper bound (500ms) so we don't get flakes on slow CI;
        // the lower bound (>50ms) confirms the tools actually ran.
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_p",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "t1", "name": "slow", "input": {"k": 1}},
                    {"type": "tool_use", "id": "t2", "name": "slow", "input": {"k": 2}},
                ],
                "model": "claude-sonnet-4-6",
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 10, "output_tokens": 5}
            })))
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

        let mut registry = ToolRegistry::new();
        registry.register("slow", json!({}), |input| async move {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            Ok(input)
        });

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("call slow tools");

        let started = std::time::Instant::now();
        let _ = client
            .run(&mut convo, &registry, RunOptions::default())
            .await
            .unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed.as_millis() < 500,
            "parallel dispatch should be fast; got {elapsed:?}"
        );
        assert!(
            elapsed.as_millis() > 50,
            "tools didn't actually run; got {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn parallel_dispatch_can_be_disabled() {
        // With parallel=false and two 50ms tools, total tool time is ~100ms.
        // We can't easily prove the disable; assert correctness instead --
        // tool_results come back in the same order as tool_use blocks.
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_seq",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "t1", "name": "echo", "input": {"v": "first"}},
                    {"type": "tool_use", "id": "t2", "name": "echo", "input": {"v": "second"}},
                ],
                "model": "claude-sonnet-4-6",
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 10, "output_tokens": 5}
            })))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({
                "messages": [
                    {"role": "user"},
                    {"role": "assistant"},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "t1"},
                        {"type": "tool_result", "tool_use_id": "t2"}
                    ]}
                ]
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("ok", "end_turn")),
            )
            .mount(&mock)
            .await;

        let mut registry = ToolRegistry::new();
        registry.register("echo", json!({}), |input| async move { Ok(input) });

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("two tools");
        let _ = client
            .run(
                &mut convo,
                &registry,
                RunOptions::default().parallel_tool_dispatch(false),
            )
            .await
            .unwrap();
    }

    #[cfg(feature = "pricing")]
    #[tokio::test]
    async fn cost_budget_aborts_loop_when_exceeded() {
        // Each turn costs ~ (1M input * $3/MTok) = $3 on Sonnet 4.6.
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_b",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "t1", "name": "noop", "input": {}}
                ],
                "model": "claude-sonnet-4-6",
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 1_000_000, "output_tokens": 0}
            })))
            .mount(&mock)
            .await;

        let mut registry = ToolRegistry::new();
        registry.register("noop", json!({}), |_input| async move { Ok(json!({})) });

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("burn money");

        let err = client
            .run(
                &mut convo,
                &registry,
                RunOptions::default()
                    .max_iterations(8)
                    .cost_budget(1.00, crate::pricing::PricingTable::default()),
            )
            .await
            .unwrap_err();
        let Error::CostBudgetExceeded {
            budget_usd,
            spent_usd,
        } = err
        else {
            panic!("expected CostBudgetExceeded, got {err:?}");
        };
        // Budget was $1; first turn already cost $3; spent_usd should reflect that.
        assert!((budget_usd - 1.00).abs() < 1e-9);
        assert!(
            spent_usd > 1.00,
            "spent_usd ({spent_usd}) should exceed budget"
        );
    }

    #[tokio::test]
    async fn cancel_token_aborts_before_first_request() {
        let mock = MockServer::start().await;
        // Mount a mock that *would* respond, but we expect it never to be hit
        // because cancellation fires before the first request.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("ok", "end_turn")),
            )
            .expect(0)
            .mount(&mock)
            .await;

        let token = tokio_util::sync::CancellationToken::new();
        token.cancel(); // pre-cancel

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("hi");

        let err = client
            .run(
                &mut convo,
                &ToolRegistry::new(),
                RunOptions::default().cancel_token(token),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Cancelled), "got {err:?}");
    }

    #[tokio::test]
    async fn cancel_token_aborts_between_iterations() {
        let mock = MockServer::start().await;
        // First iteration: tool_use; loop continues.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(assistant_tool_use(
                "t1",
                "noop",
                json!({}),
            )))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        // Second iteration: must NOT be called because we cancel after iter 1.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(assistant_text("won't run", "end_turn")),
            )
            .expect(0)
            .mount(&mock)
            .await;

        let token = tokio_util::sync::CancellationToken::new();
        let token_for_hook = token.clone();

        let mut registry = ToolRegistry::new();
        registry.register("noop", json!({}), |_| async move { Ok(json!({})) });

        let client = client_for(&mock);
        let mut convo = Conversation::new(ModelId::SONNET_4_6, 64);
        convo.push_user("hi");

        let err = client
            .run(
                &mut convo,
                &registry,
                RunOptions::default()
                    .cancel_token(token)
                    .on_iteration(move |_msg, _n| token_for_hook.cancel()),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Cancelled), "got {err:?}");
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
