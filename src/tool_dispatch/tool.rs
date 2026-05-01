//! The [`Tool`] async trait and its companion [`ToolError`].
//!
//! [`Tool`] is the foundation for every tool-dispatch shape v0.2 ships:
//! direct trait implementations, closure adapters via `FnTool` (#19), and
//! schemars-driven typed handlers (#21) all reduce to a `Tool` impl.

use async_trait::async_trait;

/// A tool the model can invoke during generation.
///
/// # Contract
///
/// - **`name()`** returns a stable identifier. Names must be unique within
///   a single registry; the model uses them to refer to specific tools.
/// - **`schema()`** returns a JSON Schema describing the tool's input. The
///   schema is sent to the model as part of the request and is used to
///   guide what `invoke` will receive. Must be valid JSON Schema.
/// - **`invoke(input)`** runs the tool. `input` is the raw `Value` the
///   model produced and is *not* validated against the schema by the SDK
///   -- impls should validate themselves and return [`ToolError::InvalidInput`]
///   on failure. `invoke` may take arbitrary time; the agent-loop runner
///   in #20 supports per-iteration timeouts.
///
/// All methods take `&self` so a single instance can be shared via `Arc`.
/// The trait is `Send + Sync + 'static` so tools can live in concurrent
/// contexts.
///
/// # Example
///
/// ```
/// use async_trait::async_trait;
/// use claude_api::tool_dispatch::{Tool, ToolError};
/// use serde_json::{json, Value};
///
/// struct AddTool;
///
/// #[async_trait]
/// impl Tool for AddTool {
///     fn name(&self) -> &str { "add" }
///     fn schema(&self) -> Value {
///         json!({
///             "type": "object",
///             "properties": {
///                 "a": {"type": "number"},
///                 "b": {"type": "number"}
///             },
///             "required": ["a", "b"]
///         })
///     }
///     async fn invoke(&self, input: Value) -> Result<Value, ToolError> {
///         let a = input.get("a").and_then(Value::as_f64)
///             .ok_or_else(|| ToolError::invalid_input("missing 'a'"))?;
///         let b = input.get("b").and_then(Value::as_f64)
///             .ok_or_else(|| ToolError::invalid_input("missing 'b'"))?;
///         Ok(json!({"sum": a + b}))
///     }
/// }
/// ```
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    /// Stable identifier the model uses to refer to this tool.
    fn name(&self) -> &str;

    /// JSON Schema describing the tool's expected input.
    fn schema(&self) -> serde_json::Value;

    /// Optional human-readable description; helps the model decide when to
    /// invoke. Default returns `None`.
    fn description(&self) -> Option<&str> {
        None
    }

    /// Run the tool with the model-supplied input and return its result.
    async fn invoke(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError>;
}

/// Errors a [`Tool`] implementation can return.
///
/// Construct via [`Self::invalid_input`] for caller-side validation
/// failures (string message, surfaced back to the model) or
/// [`Self::execution`] to wrap any underlying error type that implements
/// [`std::error::Error`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ToolError {
    /// The model-supplied input did not satisfy the tool's schema or
    /// other validation rules. The string is surfaced back to the model
    /// as the `tool_result` content.
    #[error("invalid tool input: {0}")]
    InvalidInput(String),

    /// The tool ran but its underlying operation failed.
    #[error("tool execution failed: {0}")]
    Execution(Box<dyn std::error::Error + Send + Sync>),

    /// A registry was asked to dispatch a tool name it doesn't know.
    /// Surfaced by `ToolRegistry::dispatch` (lands in #19).
    #[error("no tool registered with name '{name}'")]
    Unknown {
        /// Tool name the registry was asked to dispatch.
        name: String,
    },
}

impl ToolError {
    /// Build an [`InvalidInput`](Self::InvalidInput) error from a message.
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    /// Build an [`Execution`](Self::Execution) error wrapping any error type.
    pub fn execution<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Execution(Box::new(err))
    }
}

/// Verdict from a [`ToolApprover`] for a single `tool_use` invocation.
///
/// Approvers are consulted by [`crate::Client::run`] *before* each tool
/// dispatch, so users can gate side-effecting tools behind an interactive
/// confirmation, a policy check, an input rewriter, or a static
/// allowlist.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    /// Proceed with the tool dispatch unchanged.
    Approve,
    /// Proceed, but substitute a different `input` (the model's original
    /// payload is discarded). Useful for sanitizing arguments before the
    /// tool runs (path scrubbing, scope clamping, etc.).
    ApproveWithInput(serde_json::Value),
    /// Skip the tool dispatch entirely and return `value` as the
    /// `tool_result` content (with no `is_error` flag). Useful for
    /// stubbing tools in tests or short-circuiting expensive calls when
    /// the answer is already known.
    Substitute(serde_json::Value),
    /// Skip the tool dispatch. The supplied `reason` is returned to the
    /// model as the `tool_result` content (with `is_error = true`) so
    /// the model can choose how to recover.
    Deny(String),
    /// Abort the entire agent loop. Surfaces as
    /// [`crate::Error::ToolApprovalStopped`] from `Client::run`.
    Stop(String),
}

/// Async-callable predicate consulted before each tool dispatch.
///
/// Implement this trait for stateful approvers, or use the closure
/// adapter [`fn_approver`] / [`RunOptions::with_approver_fn`].
#[async_trait]
pub trait ToolApprover: Send + Sync + 'static {
    /// Inspect a pending tool dispatch and return a verdict.
    async fn approve(&self, tool_name: &str, input: &serde_json::Value) -> ApprovalDecision;
}

/// Wrap an async closure into a [`ToolApprover`].
#[must_use]
pub fn fn_approver<F, Fut>(handler: F) -> std::sync::Arc<dyn ToolApprover>
where
    F: Fn(&str, &serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ApprovalDecision> + Send + 'static,
{
    std::sync::Arc::new(FnApprover { handler })
}

struct FnApprover<F> {
    handler: F,
}

#[async_trait]
impl<F, Fut> ToolApprover for FnApprover<F>
where
    F: Fn(&str, &serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ApprovalDecision> + Send + 'static,
{
    async fn approve(&self, tool_name: &str, input: &serde_json::Value) -> ApprovalDecision {
        (self.handler)(tool_name, input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Arc;

    struct AddTool;

    #[async_trait]
    impl Tool for AddTool {
        // Trait dictates the return type; clippy can't tell we're returning
        // a literal versus a stored String, so allow the lint locally.
        #[allow(clippy::unnecessary_literal_bound)]
        fn name(&self) -> &str {
            "add"
        }
        #[allow(clippy::unnecessary_literal_bound)]
        fn description(&self) -> Option<&str> {
            Some("Add two numbers and return the sum.")
        }
        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "a": {"type": "number"},
                    "b": {"type": "number"}
                },
                "required": ["a", "b"]
            })
        }
        async fn invoke(&self, input: Value) -> Result<Value, ToolError> {
            let a = input
                .get("a")
                .and_then(Value::as_f64)
                .ok_or_else(|| ToolError::invalid_input("missing 'a'"))?;
            let b = input
                .get("b")
                .and_then(Value::as_f64)
                .ok_or_else(|| ToolError::invalid_input("missing 'b'"))?;
            Ok(json!({"sum": a + b}))
        }
    }

    #[tokio::test]
    async fn manual_impl_round_trips_a_value() {
        let tool = AddTool;
        let result = tool.invoke(json!({"a": 2, "b": 3})).await.unwrap();
        assert_eq!(result, json!({"sum": 5.0}));
    }

    #[tokio::test]
    async fn trait_object_dispatch_works() {
        // Critical: dyn Tool must work for ToolRegistry to hold heterogeneous tools.
        let tool: Arc<dyn Tool> = Arc::new(AddTool);
        assert_eq!(tool.name(), "add");
        assert_eq!(
            tool.description(),
            Some("Add two numbers and return the sum.")
        );
        assert!(tool.schema().is_object());
        let result = tool.invoke(json!({"a": 4, "b": 1})).await.unwrap();
        assert_eq!(result["sum"], 5.0);
    }

    #[tokio::test]
    async fn invalid_input_propagates_message() {
        let tool = AddTool;
        let err = tool.invoke(json!({"a": 1})).await.unwrap_err();
        let ToolError::InvalidInput(msg) = err else {
            panic!("expected InvalidInput");
        };
        assert!(msg.contains("'b'"), "{msg}");
    }

    #[test]
    fn invalid_input_constructor_takes_string_or_str() {
        let _ = ToolError::invalid_input("plain str");
        let _ = ToolError::invalid_input(String::from("owned"));
    }

    #[test]
    fn execution_wraps_any_std_error() {
        let inner = std::io::Error::other("disk on fire");
        let err = ToolError::execution(inner);
        let display = format!("{err}");
        assert!(display.contains("disk on fire"), "{display}");
        let ToolError::Execution(_) = err else {
            panic!("expected Execution");
        };
    }

    #[test]
    fn tool_is_send_and_sync() {
        // Compile-time check: dyn Tool must be Send + Sync to live in async tasks.
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Tool>();
    }
}
