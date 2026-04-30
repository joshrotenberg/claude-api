//! [`ToolRegistry`] and the [`FnTool`] closure adapter.
//!
//! The registry holds heterogeneous [`Tool`] implementations behind
//! `Arc<dyn Tool>` and supports two registration shapes:
//!
//! - [`ToolRegistry::register_tool`] takes anything that implements [`Tool`]
//!   directly.
//! - [`ToolRegistry::register`] takes a closure plus name/schema; the
//!   closure is wrapped in an internal [`FnTool`] that implements [`Tool`].
//!
//! Both reduce to the same `Arc<dyn Tool>`, so the agent loop runner
//! (#20) and the model's tool list ([`ToolRegistry::to_messages_tools`])
//! treat them identically.

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;

use crate::messages::tools::{CustomTool, Tool as MessagesTool};
use crate::tool_dispatch::tool::{Tool, ToolError};

/// In-memory registry of tools keyed by name.
///
/// Names must be unique; registering twice with the same name **replaces**
/// the existing entry. Use [`Self::contains`] to check first when overwrite
/// is undesired.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a value that implements [`Tool`] directly. Useful for tools
    /// that have their own state or non-trivial logic worth giving a
    /// dedicated type.
    pub fn register_tool<T: Tool>(&mut self, tool: T) -> &mut Self {
        let name = tool.name().to_owned();
        self.tools.insert(name, Arc::new(tool));
        self
    }

    /// Register a closure-based tool. The closure receives the model's raw
    /// input as a [`serde_json::Value`] and returns the tool result. Use
    /// [`ToolError::invalid_input`] for input-shape failures and
    /// [`ToolError::execution`] to wrap any other error type.
    ///
    /// # Example
    ///
    /// ```
    /// use claude_api::tool_dispatch::ToolRegistry;
    /// use serde_json::json;
    ///
    /// let mut registry = ToolRegistry::new();
    /// registry.register(
    ///     "echo",
    ///     json!({"type": "object", "properties": {"text": {"type": "string"}}}),
    ///     |input| async move { Ok(input) },
    /// );
    /// assert!(registry.contains("echo"));
    /// ```
    pub fn register<F, Fut>(
        &mut self,
        name: impl Into<String>,
        schema: serde_json::Value,
        handler: F,
    ) -> &mut Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
    {
        let name = name.into();
        let tool = FnTool::new(name.clone(), schema, handler);
        self.tools.insert(name, Arc::new(tool));
        self
    }

    /// Like [`Self::register`] but also attaches a description.
    pub fn register_described<F, Fut>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        schema: serde_json::Value,
        handler: F,
    ) -> &mut Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
    {
        let name = name.into();
        let mut tool = FnTool::new(name.clone(), schema, handler);
        tool.description = Some(description.into());
        self.tools.insert(name, Arc::new(tool));
        self
    }

    /// Borrow a registered tool by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Whether a tool with the given name is registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Iterator over registered tool names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }

    /// Build the [`Vec<MessagesTool>`](crate::messages::tools::Tool) you
    /// pass to `CreateMessageRequestBuilder::tools`. Includes name,
    /// description, and schema for every registered tool.
    #[must_use]
    pub fn to_messages_tools(&self) -> Vec<MessagesTool> {
        self.tools
            .values()
            .map(|t| {
                let mut ct = CustomTool::new(t.name(), t.schema());
                if let Some(desc) = t.description() {
                    ct = ct.description(desc);
                }
                MessagesTool::Custom(ct)
            })
            .collect()
    }

    /// Look up a tool by name and invoke it with the given input.
    ///
    /// Returns [`ToolError::Unknown`] if no tool by that name is registered.
    /// Other errors are propagated from the tool's `invoke` impl.
    pub async fn dispatch(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let tool = self.tools.get(name).ok_or_else(|| ToolError::Unknown {
            name: name.to_owned(),
        })?;
        tool.invoke(input).await
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Tools don't necessarily implement Debug; show names only.
        f.debug_struct("ToolRegistry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Internal adapter: wraps a closure and exposes it through the [`Tool`]
/// trait. Created by [`ToolRegistry::register`] and
/// [`ToolRegistry::register_described`].
pub struct FnTool<F, Fut>
where
    F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
{
    name: String,
    schema: serde_json::Value,
    description: Option<String>,
    handler: F,
    _phantom: PhantomData<fn() -> Fut>,
}

impl<F, Fut> FnTool<F, Fut>
where
    F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
{
    /// Build an `FnTool` from a name, JSON schema, and async closure.
    pub fn new(name: impl Into<String>, schema: serde_json::Value, handler: F) -> Self {
        Self {
            name: name.into(),
            schema,
            description: None,
            handler,
            _phantom: PhantomData,
        }
    }

    /// Attach a description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

#[async_trait]
impl<F, Fut> Tool for FnTool<F, Fut>
where
    F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    async fn invoke(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        (self.handler)(input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tools::Tool as MessagesTool;
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};

    fn echo_schema() -> Value {
        json!({"type": "object", "properties": {"text": {"type": "string"}}})
    }

    // A trait-impl tool, so we cover both registration paths.
    struct UpperTool;

    #[async_trait]
    impl Tool for UpperTool {
        // Trait dictates the return type; literal-vs-stored is up to the impl.
        #[allow(clippy::unnecessary_literal_bound)]
        fn name(&self) -> &str {
            "upper"
        }
        fn schema(&self) -> Value {
            json!({"type": "object", "properties": {"text": {"type": "string"}}})
        }
        async fn invoke(&self, input: Value) -> Result<Value, ToolError> {
            let s = input
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::invalid_input("missing 'text'"))?;
            Ok(json!({"upper": s.to_uppercase()}))
        }
    }

    #[tokio::test]
    async fn register_and_dispatch_closure_tool() {
        let mut registry = ToolRegistry::new();
        registry.register("echo", echo_schema(), |input| async move { Ok(input) });

        assert!(registry.contains("echo"));
        assert_eq!(registry.len(), 1);

        let result = registry
            .dispatch("echo", json!({"text": "hi"}))
            .await
            .unwrap();
        assert_eq!(result, json!({"text": "hi"}));
    }

    #[tokio::test]
    async fn register_and_dispatch_trait_tool() {
        let mut registry = ToolRegistry::new();
        registry.register_tool(UpperTool);

        let result = registry
            .dispatch("upper", json!({"text": "rust"}))
            .await
            .unwrap();
        assert_eq!(result, json!({"upper": "RUST"}));
    }

    #[tokio::test]
    async fn closure_and_trait_tools_coexist() {
        let mut registry = ToolRegistry::new();
        registry
            .register_tool(UpperTool)
            .register("echo", echo_schema(), |input| async move { Ok(input) });

        assert_eq!(registry.len(), 2);
        let names: std::collections::HashSet<_> = registry.names().collect();
        assert!(names.contains("upper"));
        assert!(names.contains("echo"));

        let r1 = registry
            .dispatch("upper", json!({"text": "ok"}))
            .await
            .unwrap();
        let r2 = registry
            .dispatch("echo", json!({"text": "ok"}))
            .await
            .unwrap();
        assert_eq!(r1, json!({"upper": "OK"}));
        assert_eq!(r2, json!({"text": "ok"}));
    }

    #[tokio::test]
    async fn dispatch_unknown_returns_unknown_error() {
        let registry = ToolRegistry::new();
        let err = registry.dispatch("nope", json!({})).await.unwrap_err();
        let ToolError::Unknown { name } = err else {
            panic!("expected Unknown variant");
        };
        assert_eq!(name, "nope");
    }

    #[tokio::test]
    async fn dispatch_propagates_invalid_input_error_from_tool() {
        let mut registry = ToolRegistry::new();
        registry.register_tool(UpperTool);
        let err = registry.dispatch("upper", json!({})).await.unwrap_err();
        let ToolError::InvalidInput(msg) = err else {
            panic!("expected InvalidInput");
        };
        assert!(msg.contains("'text'"));
    }

    #[tokio::test]
    async fn duplicate_register_replaces_previous_entry() {
        let mut registry = ToolRegistry::new();
        registry.register("dup", echo_schema(), |_| async move {
            Ok(json!({"version": "first"}))
        });
        registry.register("dup", echo_schema(), |_| async move {
            Ok(json!({"version": "second"}))
        });
        assert_eq!(registry.len(), 1);
        let r = registry.dispatch("dup", json!({})).await.unwrap();
        assert_eq!(r, json!({"version": "second"}));
    }

    #[test]
    fn to_messages_tools_includes_name_schema_and_description() {
        let mut registry = ToolRegistry::new();
        registry.register_tool(UpperTool).register_described(
            "echo",
            "Returns its input verbatim.",
            echo_schema(),
            |input| async move { Ok(input) },
        );

        let tools = registry.to_messages_tools();
        assert_eq!(tools.len(), 2);

        // Every entry is a Custom tool with the right name and schema.
        let mut by_name: std::collections::HashMap<String, MessagesTool> =
            std::collections::HashMap::new();
        for t in tools {
            let MessagesTool::Custom(ct) = &t else {
                panic!("expected custom variant");
            };
            by_name.insert(ct.name.clone(), t);
        }

        let MessagesTool::Custom(echo) = by_name.get("echo").unwrap() else {
            panic!("expected echo Custom");
        };
        assert_eq!(
            echo.description.as_deref(),
            Some("Returns its input verbatim.")
        );
        assert!(echo.input_schema.is_object());

        let MessagesTool::Custom(upper) = by_name.get("upper").unwrap() else {
            panic!("expected upper Custom");
        };
        assert_eq!(upper.description, None); // UpperTool didn't override description
    }

    #[tokio::test]
    async fn registry_works_through_dyn_dispatch() {
        // Sanity check: tools live behind Arc<dyn Tool> and dispatch correctly
        // through trait objects.
        let mut registry = ToolRegistry::new();
        registry.register_tool(UpperTool);

        let tool: &Arc<dyn Tool> = registry.get("upper").unwrap();
        let r = tool.invoke(json!({"text": "abc"})).await.unwrap();
        assert_eq!(r, json!({"upper": "ABC"}));
    }

    #[test]
    fn debug_impl_lists_tool_names() {
        let mut registry = ToolRegistry::new();
        registry.register_tool(UpperTool);
        let dbg = format!("{registry:?}");
        assert!(dbg.contains("upper"), "{dbg}");
    }

    #[test]
    fn registry_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ToolRegistry>();
    }
}
