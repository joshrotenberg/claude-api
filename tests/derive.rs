//! Integration tests for `#[derive(Tool)]`.

#![cfg(all(feature = "derive", feature = "async"))]

use claude_api::derive::Tool;
use claude_api::tool_dispatch::{Tool as _, ToolError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// Get the current weather for a city.
#[derive(Deserialize, JsonSchema, Tool)]
struct GetWeather {
    /// City name.
    city: String,
}

impl GetWeather {
    #[allow(clippy::unused_async)]
    async fn run(self) -> Result<serde_json::Value, ToolError> {
        Ok(json!({"temp": 72, "city": self.city}))
    }
}

#[derive(Deserialize, JsonSchema, Tool)]
#[tool(
    name = "lookup",
    description = "Custom override for both name and description."
)]
struct LookupArgs {
    /// Some query.
    q: String,
}

impl LookupArgs {
    #[allow(clippy::unused_async)]
    async fn run(self) -> Result<serde_json::Value, ToolError> {
        Ok(json!({"q": self.q, "matched": true}))
    }
}

#[tokio::test]
async fn derived_tool_uses_snake_case_type_name_by_default() {
    let tool = GetWeather::tool();
    assert_eq!(tool.name(), "get_weather");
}

#[tokio::test]
async fn derived_tool_uses_first_doc_line_as_description() {
    let tool = GetWeather::tool();
    assert_eq!(
        tool.description(),
        Some("Get the current weather for a city.")
    );
}

#[tokio::test]
async fn derived_tool_emits_object_schema_with_required_fields() {
    let tool = GetWeather::tool();
    let schema = tool.schema();
    assert_eq!(schema["type"], "object");
    let required = schema["required"].as_array().unwrap();
    assert!(
        required.iter().any(|v| v == "city"),
        "expected `city` in required: {required:?}"
    );
}

#[tokio::test]
async fn derived_tool_invoke_runs_user_handler() {
    let tool = GetWeather::tool();
    let result = tool.invoke(json!({"city": "Paris"})).await.unwrap();
    assert_eq!(result["temp"], 72);
    assert_eq!(result["city"], "Paris");
}

#[tokio::test]
async fn derived_tool_invoke_returns_invalid_input_on_bad_json() {
    let tool = GetWeather::tool();
    let err = tool.invoke(json!({"wrong_field": 1})).await.unwrap_err();
    let ToolError::InvalidInput(msg) = err else {
        panic!("expected InvalidInput");
    };
    assert!(
        msg.contains("get_weather"),
        "error message should reference tool name: {msg}"
    );
}

#[tokio::test]
async fn derived_tool_attribute_overrides_name_and_description() {
    let tool = LookupArgs::tool();
    assert_eq!(tool.name(), "lookup");
    assert_eq!(
        tool.description(),
        Some("Custom override for both name and description.")
    );
}

#[tokio::test]
async fn derived_tool_dispatches_through_registry() {
    use claude_api::tool_dispatch::ToolRegistry;

    let mut registry = ToolRegistry::new();
    registry.register_tool(GetWeather::tool());
    registry.register_tool(LookupArgs::tool());

    let r1 = registry
        .dispatch("get_weather", json!({"city": "Tokyo"}))
        .await
        .unwrap();
    assert_eq!(r1["city"], "Tokyo");

    let r2 = registry
        .dispatch("lookup", json!({"q": "rust"}))
        .await
        .unwrap();
    assert_eq!(r2["matched"], true);
}
