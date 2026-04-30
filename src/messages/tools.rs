//! Tool definitions and tool-choice configuration.
//!
//! [`Tool`] is the unit you pass to `messages.create(...)` to expose either
//! a user-defined function ([`CustomTool`]) or a server-side built-in
//! ([`BuiltinTool`]) like web search or code execution. [`ToolChoice`]
//! controls whether and which tool the model must invoke.

use serde::{Deserialize, Serialize};

use crate::messages::cache::CacheControl;

/// A tool definition the model can call during generation.
///
/// The wire form is [`untagged`](https://serde.rs/enum-representations.html#untagged):
/// custom tools are recognized by the presence of an `input_schema` field;
/// anything else is treated as a built-in tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tool {
    /// A user-defined tool with a JSON Schema for input validation.
    Custom(CustomTool),
    /// A server-side built-in tool (web search, computer use, code execution, etc.).
    ///
    /// Modeled as raw JSON in v0.1 -- typed wrappers for specific built-ins
    /// land in v0.4.
    Builtin(BuiltinTool),
}

impl Tool {
    /// Convenience constructor for a custom tool with a manually provided JSON Schema.
    pub fn custom(name: impl Into<String>, input_schema: serde_json::Value) -> Self {
        Self::Custom(CustomTool {
            name: name.into(),
            description: None,
            input_schema,
            cache_control: None,
        })
    }

    /// Convenience constructor for a built-in tool from a raw JSON object.
    ///
    /// ```
    /// use claude_api::messages::tools::Tool;
    /// let t = Tool::builtin(serde_json::json!({
    ///     "type": "web_search_20250305",
    ///     "name": "web_search"
    /// }));
    /// assert!(matches!(t, Tool::Builtin(_)));
    /// ```
    pub fn builtin(value: serde_json::Value) -> Self {
        Self::Builtin(BuiltinTool(value))
    }

    /// Build a custom tool whose input schema is derived from a Rust type via [`schemars`].
    ///
    /// # Panics
    ///
    /// Panics only if the generated `RootSchema` fails to JSON-serialize,
    /// which `schemars` guarantees not to happen for any type that implements
    /// [`schemars::JsonSchema`].
    #[cfg(feature = "schemars-tools")]
    #[cfg_attr(docsrs, doc(cfg(feature = "schemars-tools")))]
    pub fn from_schemars<T: schemars::JsonSchema>(name: impl Into<String>) -> Self {
        let schema = schemars::gen::SchemaGenerator::default().into_root_schema_for::<T>();
        let schema_value =
            serde_json::to_value(schema).expect("RootSchema is always JSON-serializable");
        Self::Custom(CustomTool {
            name: name.into(),
            description: None,
            input_schema: schema_value,
            cache_control: None,
        })
    }
}

/// User-defined tool with a JSON Schema describing its input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CustomTool {
    /// Tool name. Must be unique within a request.
    pub name: String,
    /// Human-readable description; helps the model decide when to invoke.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the tool's input arguments.
    pub input_schema: serde_json::Value,
    /// Optional cache breakpoint to apply to this tool's definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl CustomTool {
    /// Construct a custom tool with no description and no cache control.
    pub fn new(name: impl Into<String>, input_schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            input_schema,
            cache_control: None,
        }
    }

    /// Set the tool description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Apply a cache breakpoint at this tool's definition.
    #[must_use]
    pub fn cache_control(mut self, cache_control: CacheControl) -> Self {
        self.cache_control = Some(cache_control);
        self
    }

    /// Shorthand: apply an ephemeral cache breakpoint at the default
    /// (5-minute) TTL.
    #[must_use]
    pub fn with_ephemeral_cache(self) -> Self {
        self.cache_control(CacheControl::ephemeral())
    }
}

/// Server-side built-in tool. Wraps the raw wire JSON in v0.1; typed
/// variants for `web_search`, `computer`, `code_execution`, `bash`, and
/// `text_editor` land in v0.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BuiltinTool(pub serde_json::Value);

impl BuiltinTool {
    /// Wrap a JSON object describing a built-in tool.
    pub fn new(value: serde_json::Value) -> Self {
        Self(value)
    }
}

/// How (or whether) the model should invoke a tool on this turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolChoice {
    /// Default: the model decides whether and which tool to use.
    Auto {
        /// Set `true` to force serial tool calls when the model decides to use tools.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disable_parallel_tool_use: Option<bool>,
    },
    /// The model must use a tool, but may choose which one.
    Any {
        /// Set `true` to force serial tool calls.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disable_parallel_tool_use: Option<bool>,
    },
    /// The model must use the named tool.
    Tool {
        /// Tool name the model must invoke.
        name: String,
        /// Set `true` to force a single tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disable_parallel_tool_use: Option<bool>,
    },
    /// The model must not use any tool.
    None,
}

impl ToolChoice {
    /// Default `auto` choice with no parallelism override.
    #[must_use]
    pub fn auto() -> Self {
        Self::Auto {
            disable_parallel_tool_use: None,
        }
    }

    /// `any` -- model must invoke some tool of its choice.
    #[must_use]
    pub fn any() -> Self {
        Self::Any {
            disable_parallel_tool_use: None,
        }
    }

    /// Force the model to invoke the named tool.
    #[must_use]
    pub fn tool(name: impl Into<String>) -> Self {
        Self::Tool {
            name: name.into(),
            disable_parallel_tool_use: None,
        }
    }

    /// Forbid all tool use.
    #[must_use]
    pub fn none() -> Self {
        Self::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn custom_tool_round_trips() {
        let t = Tool::Custom(
            CustomTool::new(
                "get_weather",
                json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            )
            .description("Look up the weather"),
        );
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(
            v,
            json!({
                "name": "get_weather",
                "description": "Look up the weather",
                "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}
            })
        );
        let parsed: Tool = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn custom_tool_with_cache_control_round_trips() {
        let t = Tool::Custom(
            CustomTool::new("noop", json!({"type": "object"}))
                .cache_control(CacheControl::ephemeral()),
        );
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(
            v,
            json!({
                "name": "noop",
                "input_schema": {"type": "object"},
                "cache_control": {"type": "ephemeral"}
            })
        );
        let parsed: Tool = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn builtin_tool_round_trips_as_raw_json() {
        let raw = json!({"type": "web_search_20250305", "name": "web_search", "max_uses": 5});
        let t = Tool::builtin(raw.clone());
        let serialized = serde_json::to_value(&t).unwrap();
        assert_eq!(serialized, raw, "Builtin must serialize transparently");
        let parsed: Tool = serde_json::from_value(serialized).unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn untagged_enum_disambiguates_custom_from_builtin() {
        // Has `input_schema` -> Custom
        let custom: Tool = serde_json::from_value(json!({
            "name": "x",
            "input_schema": {"type": "object"}
        }))
        .unwrap();
        assert!(matches!(custom, Tool::Custom(_)));

        // No `input_schema` -> Builtin
        let builtin: Tool = serde_json::from_value(json!({
            "type": "web_search_20250305",
            "name": "web_search"
        }))
        .unwrap();
        assert!(matches!(builtin, Tool::Builtin(_)));
    }

    #[test]
    fn tool_choice_auto_round_trips() {
        let c = ToolChoice::auto();
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v, json!({"type": "auto"}));
        let parsed: ToolChoice = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn tool_choice_any_with_no_parallel_round_trips() {
        let c = ToolChoice::Any {
            disable_parallel_tool_use: Some(true),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v, json!({"type": "any", "disable_parallel_tool_use": true}));
        let parsed: ToolChoice = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn tool_choice_specific_tool_round_trips() {
        let c = ToolChoice::tool("get_weather");
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v, json!({"type": "tool", "name": "get_weather"}));
        let parsed: ToolChoice = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn tool_choice_none_round_trips() {
        let c = ToolChoice::none();
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v, json!({"type": "none"}));
        let parsed: ToolChoice = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }

    #[cfg(feature = "schemars-tools")]
    #[test]
    fn from_schemars_builds_custom_tool() {
        #[derive(schemars::JsonSchema, serde::Deserialize)]
        #[allow(dead_code)]
        struct Args {
            city: String,
            units: Option<String>,
        }

        let t = Tool::from_schemars::<Args>("get_weather");
        match t {
            Tool::Custom(c) => {
                assert_eq!(c.name, "get_weather");
                // Schema should be a JSON object describing the type.
                assert!(c.input_schema.is_object());
            }
            Tool::Builtin(_) => panic!("expected Custom"),
        }
    }
}
