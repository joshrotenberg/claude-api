//! Tool definitions and tool-choice configuration.
//!
//! [`Tool`] is the unit you pass to `messages.create(...)` to expose either
//! a user-defined function ([`CustomTool`]) or a server-side built-in
//! ([`BuiltinTool`]) like web search or code execution. [`ToolChoice`]
//! controls whether and which tool the model must invoke.
//!
//! For high-level dispatch (registry, parallel calls, agent loop) see
//! [`crate::tool_dispatch`]. For `#[derive(Tool)]` see `claude-api-derive`.
//!
//! # Define and send a custom tool
//!
//! ```no_run
//! use claude_api::{Client, messages::{CreateMessageRequest, Tool, CustomTool},
//!     types::ModelId};
//! use serde_json::json;
//! # async fn run() -> Result<(), claude_api::Error> {
//! let weather = Tool::Custom(
//!     CustomTool::new(
//!         "get_weather",
//!         json!({
//!             "type": "object",
//!             "properties": {"city": {"type": "string"}},
//!             "required": ["city"]
//!         }),
//!     )
//!     .description("Return the current weather for a city."),
//! );
//! let client = Client::new(std::env::var("ANTHROPIC_API_KEY").unwrap());
//! let resp = client
//!     .messages()
//!     .create(
//!         CreateMessageRequest::builder()
//!             .model(ModelId::SONNET_4_6)
//!             .max_tokens(512)
//!             .tools(vec![weather])
//!             .user("What's the weather in Tokyo?")
//!             .build()?,
//!     )
//!     .await?;
//! println!("{:?}", resp.stop_reason);
//! # Ok(())
//! # }
//! ```

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
    /// Typed via [`KnownBuiltinTool`] with an `Other(Value)` arm for
    /// forward-compatibility with new tools / new tool versions.
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

    /// Escape hatch: build a `Tool::Builtin` from raw JSON. Use the typed
    /// constructors ([`Self::web_search`], [`Self::computer`], [`Self::bash`],
    /// [`Self::text_editor`], [`Self::code_execution`]) instead when the
    /// tool type is known to the SDK; this is for unknown / future tools.
    ///
    /// ```
    /// use claude_api::messages::tools::Tool;
    /// // Unknown future tool -- round-trips through Builtin::Other.
    /// let t = Tool::builtin(serde_json::json!({
    ///     "type": "future_tool_2099",
    ///     "name": "future"
    /// }));
    /// assert!(matches!(t, Tool::Builtin(_)));
    /// ```
    pub fn builtin(value: serde_json::Value) -> Self {
        Self::Builtin(BuiltinTool::Other(value))
    }

    /// Default-config web search tool (`web_search_20250305`).
    #[must_use]
    pub fn web_search() -> Self {
        Self::Builtin(BuiltinTool::Known(KnownBuiltinTool::WebSearch20250305 {
            name: "web_search".into(),
            max_uses: None,
            allowed_domains: None,
            blocked_domains: None,
            user_location: None,
            cache_control: None,
        }))
    }

    /// Computer-use tool with the given display dimensions
    /// (`computer_20250124`).
    #[must_use]
    pub fn computer(display_width_px: u32, display_height_px: u32) -> Self {
        Self::Builtin(BuiltinTool::Known(KnownBuiltinTool::Computer20250124 {
            name: "computer".into(),
            display_width_px,
            display_height_px,
            display_number: None,
            cache_control: None,
        }))
    }

    /// Default-config bash tool (`bash_20250124`).
    #[must_use]
    pub fn bash() -> Self {
        Self::Builtin(BuiltinTool::Known(KnownBuiltinTool::Bash20250124 {
            name: "bash".into(),
            cache_control: None,
        }))
    }

    /// Default-config text editor tool (`text_editor_20250124`).
    #[must_use]
    pub fn text_editor() -> Self {
        Self::Builtin(BuiltinTool::Known(KnownBuiltinTool::TextEditor20250124 {
            name: "str_replace_editor".into(),
            cache_control: None,
        }))
    }

    /// Default-config code execution tool (`code_execution_20250825`).
    #[must_use]
    pub fn code_execution() -> Self {
        Self::Builtin(BuiltinTool::Known(
            KnownBuiltinTool::CodeExecution20250825 {
                name: "code_execution".into(),
                cache_control: None,
            },
        ))
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
        let schema = schemars::r#gen::SchemaGenerator::default().into_root_schema_for::<T>();
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

/// Server-side built-in tool from Anthropic's catalog (web search, computer
/// use, code execution, bash, text editor).
///
/// Forward-compatible wrapper around [`KnownBuiltinTool`]; unknown tool
/// types deserialize into [`BuiltinTool::Other`] preserving the raw JSON.
/// New built-in tools or new versions of existing ones (the date-stamped
/// type tags) work without an SDK update.
#[derive(Debug, Clone, PartialEq)]
pub enum BuiltinTool {
    /// A built-in tool whose `type` is recognized by this SDK version.
    Known(KnownBuiltinTool),
    /// A built-in tool whose `type` is not recognized; raw JSON preserved.
    Other(serde_json::Value),
}

/// All server-side built-in tool variants known to this SDK version.
///
/// `#[non_exhaustive]` on both the enum (so adding a variant is non-breaking)
/// and on each struct variant (so adding a field is non-breaking).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum KnownBuiltinTool {
    /// Web search tool, version `2025-03-05`. The model can issue
    /// keyword searches; results are returned as
    /// [`KnownBlock::WebSearchToolResult`](crate::messages::content::KnownBlock::WebSearchToolResult)
    /// blocks.
    #[serde(rename = "web_search_20250305")]
    WebSearch20250305 {
        /// Tool name surfaced to the model. Conventionally `"web_search"`.
        name: String,
        /// Maximum number of search requests the model may make per turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_uses: Option<u32>,
        /// If set, restrict searches to these domains.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allowed_domains: Option<Vec<String>>,
        /// If set, exclude these domains from search results.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocked_domains: Option<Vec<String>>,
        /// Approximate user location to bias results.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_location: Option<UserLocation>,
        /// Optional cache breakpoint on the tool definition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Computer-use tool, version `2025-01-24`. Lets the model take
    /// screenshots and synthesize mouse/keyboard input.
    #[serde(rename = "computer_20250124")]
    Computer20250124 {
        /// Tool name. Conventionally `"computer"`.
        name: String,
        /// Display width in pixels.
        display_width_px: u32,
        /// Display height in pixels.
        display_height_px: u32,
        /// Optional X11 display number.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_number: Option<u32>,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Bash tool, version `2025-01-24`.
    #[serde(rename = "bash_20250124")]
    Bash20250124 {
        /// Tool name. Conventionally `"bash"`.
        name: String,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Text editor tool, version `2025-01-24`.
    #[serde(rename = "text_editor_20250124")]
    TextEditor20250124 {
        /// Tool name. Conventionally `"str_replace_editor"`.
        name: String,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Code execution tool, version `2025-08-25`. Server-side sandboxed
    /// Python execution.
    #[serde(rename = "code_execution_20250825")]
    CodeExecution20250825 {
        /// Tool name. Conventionally `"code_execution"`.
        name: String,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

const KNOWN_BUILTIN_TAGS: &[&str] = &[
    "web_search_20250305",
    "computer_20250124",
    "bash_20250124",
    "text_editor_20250124",
    "code_execution_20250825",
];

impl serde::Serialize for BuiltinTool {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            BuiltinTool::Known(k) => k.serialize(s),
            BuiltinTool::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> serde::Deserialize<'de> for BuiltinTool {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        crate::forward_compat::dispatch_known_or_other(
            raw,
            KNOWN_BUILTIN_TAGS,
            BuiltinTool::Known,
            BuiltinTool::Other,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl From<KnownBuiltinTool> for BuiltinTool {
    fn from(k: KnownBuiltinTool) -> Self {
        BuiltinTool::Known(k)
    }
}

/// Approximate user location, used to bias web-search results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserLocation {
    /// Location specificity. Anthropic accepts `"approximate"`.
    #[serde(rename = "type", default = "default_user_location_kind")]
    pub kind: String,
    /// City name (English).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// Region (state, province) name (English).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// ISO 3166-1 alpha-2 country code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// IANA timezone identifier (e.g. `America/Los_Angeles`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

fn default_user_location_kind() -> String {
    "approximate".to_owned()
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
    fn unknown_builtin_round_trips_through_other() {
        // The escape-hatch path: unknown tool type stays as Builtin::Other.
        let raw = json!({"type": "future_builtin_2099", "name": "future_tool"});
        let t = Tool::builtin(raw.clone());
        let serialized = serde_json::to_value(&t).unwrap();
        assert_eq!(serialized, raw, "Other must serialize transparently");
        let parsed: Tool = serde_json::from_value(serialized).unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn known_builtin_parses_into_typed_variant() {
        let raw = json!({
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 5
        });
        let parsed: Tool = serde_json::from_value(raw).unwrap();
        match parsed {
            Tool::Builtin(BuiltinTool::Known(KnownBuiltinTool::WebSearch20250305 {
                name,
                max_uses,
                ..
            })) => {
                assert_eq!(name, "web_search");
                assert_eq!(max_uses, Some(5));
            }
            other => panic!("expected typed WebSearch20250305, got {other:?}"),
        }
    }

    #[test]
    fn web_search_default_serializes_to_minimal_wire_form() {
        let t = Tool::web_search();
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(
            v,
            json!({"type": "web_search_20250305", "name": "web_search"})
        );
    }

    #[test]
    fn web_search_with_options_round_trips() {
        let t = Tool::Builtin(BuiltinTool::Known(KnownBuiltinTool::WebSearch20250305 {
            name: "web_search".into(),
            max_uses: Some(3),
            allowed_domains: Some(vec!["wikipedia.org".into()]),
            blocked_domains: None,
            user_location: Some(UserLocation {
                kind: "approximate".into(),
                city: Some("Paris".into()),
                region: None,
                country: Some("FR".into()),
                timezone: Some("Europe/Paris".into()),
            }),
            cache_control: Some(CacheControl::ephemeral()),
        }));
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 3,
                "allowed_domains": ["wikipedia.org"],
                "user_location": {
                    "type": "approximate",
                    "city": "Paris",
                    "country": "FR",
                    "timezone": "Europe/Paris"
                },
                "cache_control": {"type": "ephemeral"}
            })
        );
        let parsed: Tool = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn computer_default_serializes_with_required_dims() {
        let t = Tool::computer(1920, 1080);
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "computer_20250124",
                "name": "computer",
                "display_width_px": 1920,
                "display_height_px": 1080
            })
        );
    }

    #[test]
    fn bash_text_editor_code_execution_defaults_serialize() {
        assert_eq!(
            serde_json::to_value(Tool::bash()).unwrap(),
            json!({"type": "bash_20250124", "name": "bash"})
        );
        assert_eq!(
            serde_json::to_value(Tool::text_editor()).unwrap(),
            json!({"type": "text_editor_20250124", "name": "str_replace_editor"})
        );
        assert_eq!(
            serde_json::to_value(Tool::code_execution()).unwrap(),
            json!({"type": "code_execution_20250825", "name": "code_execution"})
        );
    }

    #[test]
    fn malformed_known_builtin_errors_not_silent_fallthrough() {
        // Known type, but display_width_px is wrong shape.
        let raw = json!({
            "type": "computer_20250124",
            "name": "computer",
            "display_width_px": "wide",
            "display_height_px": 1080
        });
        let result: Result<Tool, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "malformed known builtin must error, not fall through to Other"
        );
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
