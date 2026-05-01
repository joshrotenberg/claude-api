//! Schemars-driven typed tool registration.
//!
//! Layers on top of [`ToolRegistry`] and the
//! [`Tool`] trait: instead of a raw `Fn(Value) -> Future` handler, you
//! supply a typed `Fn(Args) -> Future` where `Args: JsonSchema +
//! DeserializeOwned`. The schema is derived automatically; the registry
//! deserializes the model's input into `Args` before invoking, returning
//! [`ToolError::InvalidInput`] when the shape doesn't match.
//!
//! Gated on the `schemars-tools` feature.
//!
//! ```
//! use claude_api::tool_dispatch::ToolRegistry;
//! use schemars::JsonSchema;
//! use serde::Deserialize;
//! use serde_json::json;
//!
//! #[derive(JsonSchema, Deserialize)]
//! struct WeatherArgs {
//!     city: String,
//!     #[serde(default)]
//!     units: Option<String>,
//! }
//!
//! let mut registry = ToolRegistry::new();
//! registry.register_typed::<WeatherArgs, _, _>(
//!     "get_weather",
//!     |args| async move {
//!         Ok(json!({"city": args.city, "units": args.units.unwrap_or_default()}))
//!     },
//! );
//! ```

#![cfg(feature = "schemars-tools")]

use std::future::Future;
use std::marker::PhantomData;

use async_trait::async_trait;

use crate::tool_dispatch::registry::ToolRegistry;
use crate::tool_dispatch::tool::{Tool, ToolError};

/// Generate a JSON Schema for the given type via [`schemars`].
fn generate_schema_for<A: schemars::JsonSchema>() -> serde_json::Value {
    let schema = schemars::r#gen::SchemaGenerator::default().into_root_schema_for::<A>();
    serde_json::to_value(schema).expect("RootSchema is always JSON-serializable")
}

/// Typed-input adapter that implements [`Tool`] for a handler taking a
/// `JsonSchema`-deriving struct.
///
/// Constructed implicitly by [`ToolRegistry::register_typed`] /
/// [`ToolRegistry::register_typed_described`]; rarely instantiated directly.
pub struct TypedTool<A, F, Fut>
where
    A: schemars::JsonSchema + serde::de::DeserializeOwned + Send + Sync + 'static,
    F: Fn(A) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
{
    name: String,
    schema: serde_json::Value,
    description: Option<String>,
    handler: F,
    _phantom: PhantomData<fn(A) -> Fut>,
}

impl<A, F, Fut> TypedTool<A, F, Fut>
where
    A: schemars::JsonSchema + serde::de::DeserializeOwned + Send + Sync + 'static,
    F: Fn(A) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
{
    /// Build a typed tool. The schema is derived from `A` automatically.
    pub fn new(name: impl Into<String>, handler: F) -> Self {
        Self {
            name: name.into(),
            schema: generate_schema_for::<A>(),
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
impl<A, F, Fut> Tool for TypedTool<A, F, Fut>
where
    A: schemars::JsonSchema + serde::de::DeserializeOwned + Send + Sync + 'static,
    F: Fn(A) -> Fut + Send + Sync + 'static,
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
        let args = serde_json::from_value::<A>(input).map_err(|e| {
            ToolError::invalid_input(format!("input did not match schema for {}: {e}", self.name))
        })?;
        (self.handler)(args).await
    }
}

impl ToolRegistry {
    /// Register a tool with a typed input struct.
    ///
    /// The schema is generated from `A` via [`schemars`], and the model's
    /// raw `Value` input is deserialized into `A` before the handler runs.
    /// Deserialization failures surface as [`ToolError::InvalidInput`] so
    /// the model can self-correct.
    pub fn register_typed<A, F, Fut>(&mut self, name: impl Into<String>, handler: F) -> &mut Self
    where
        A: schemars::JsonSchema + serde::de::DeserializeOwned + Send + Sync + 'static,
        F: Fn(A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
    {
        let tool = TypedTool::<A, F, Fut>::new(name, handler);
        self.register_tool(tool)
    }

    /// Like [`Self::register_typed`] but also attaches a description.
    pub fn register_typed_described<A, F, Fut>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> &mut Self
    where
        A: schemars::JsonSchema + serde::de::DeserializeOwned + Send + Sync + 'static,
        F: Fn(A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ToolError>> + Send + 'static,
    {
        let tool = TypedTool::<A, F, Fut>::new(name, handler).with_description(description);
        self.register_tool(tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tools::Tool as MessagesTool;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(JsonSchema, Deserialize)]
    struct WeatherArgs {
        city: String,
        #[serde(default)]
        units: Option<String>,
    }

    #[tokio::test]
    async fn register_typed_dispatches_with_typed_args() {
        let mut registry = ToolRegistry::new();
        registry.register_typed::<WeatherArgs, _, _>("weather", |args| async move {
            Ok(json!({
                "city": args.city,
                "units": args.units.unwrap_or_else(|| "F".into())
            }))
        });

        let result = registry
            .dispatch("weather", json!({"city": "Paris"}))
            .await
            .unwrap();
        assert_eq!(result["city"], "Paris");
        assert_eq!(result["units"], "F");
    }

    #[tokio::test]
    async fn register_typed_passes_optional_fields_through() {
        let mut registry = ToolRegistry::new();
        registry.register_typed::<WeatherArgs, _, _>("weather", |args| async move {
            Ok(json!({"city": args.city, "units": args.units}))
        });
        let result = registry
            .dispatch("weather", json!({"city": "Tokyo", "units": "C"}))
            .await
            .unwrap();
        assert_eq!(result["units"], "C");
    }

    #[tokio::test]
    async fn register_typed_returns_invalid_input_on_missing_required_field() {
        let mut registry = ToolRegistry::new();
        registry.register_typed::<WeatherArgs, _, _>("weather", |args| async move {
            Ok(json!({"city": args.city}))
        });
        let err = registry.dispatch("weather", json!({})).await.unwrap_err();
        let ToolError::InvalidInput(msg) = err else {
            panic!("expected InvalidInput");
        };
        assert!(msg.contains("city"), "{msg}");
    }

    #[tokio::test]
    async fn register_typed_returns_invalid_input_on_wrong_field_type() {
        let mut registry = ToolRegistry::new();
        registry.register_typed::<WeatherArgs, _, _>("weather", |args| async move {
            Ok(json!({"city": args.city}))
        });
        // city should be string, not number
        let err = registry
            .dispatch("weather", json!({"city": 42}))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[test]
    fn register_typed_generates_schema_from_args_type() {
        let mut registry = ToolRegistry::new();
        registry.register_typed::<WeatherArgs, _, _>("weather", |_args| async move {
            Ok(serde_json::Value::Null)
        });

        let tools = registry.to_messages_tools();
        let MessagesTool::Custom(ct) = &tools[0] else {
            panic!("expected Custom");
        };
        assert!(ct.input_schema.is_object(), "schema must be a JSON object");
        // Schema should describe the city field.
        let serialized = ct.input_schema.to_string();
        assert!(
            serialized.contains("\"city\""),
            "schema must mention city: {serialized}"
        );
    }

    #[test]
    fn register_typed_described_attaches_description_to_messages_tools() {
        let mut registry = ToolRegistry::new();
        registry.register_typed_described::<WeatherArgs, _, _>(
            "weather",
            "Get the weather for a city.",
            |args| async move { Ok(json!({"city": args.city})) },
        );
        let tools = registry.to_messages_tools();
        let MessagesTool::Custom(ct) = &tools[0] else {
            panic!("expected Custom");
        };
        assert_eq!(
            ct.description.as_deref(),
            Some("Get the weather for a city.")
        );
    }

    #[tokio::test]
    async fn typed_and_closure_tools_coexist_in_one_registry() {
        let mut registry = ToolRegistry::new();
        registry
            .register_typed::<WeatherArgs, _, _>("weather", |args| async move {
                Ok(json!({"city": args.city}))
            })
            .register("echo", json!({"type": "object"}), |input| async move {
                Ok(input)
            });

        assert_eq!(registry.len(), 2);
        let r1 = registry
            .dispatch("weather", json!({"city": "Berlin"}))
            .await
            .unwrap();
        let r2 = registry.dispatch("echo", json!({"x": 1})).await.unwrap();
        assert_eq!(r1["city"], "Berlin");
        assert_eq!(r2["x"], 1);
    }
}
