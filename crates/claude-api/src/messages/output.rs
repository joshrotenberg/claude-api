//! Output configuration for the Messages API (`output_config`).
//!
//! Bundles the three generation-shaping controls the API exposes under the
//! top-level `output_config` object:
//!
//! - [`OutputFormat`] -- constrain the model's output to a JSON Schema
//!   (structured outputs). GA; no beta header.
//! - [`Effort`] -- how hard the model works before answering. GA; no beta
//!   header.
//! - [`TaskBudget`] -- an advisory token budget for an agentic loop. Beta:
//!   requires the `task-budgets-2026-03-13` header via
//!   [`ClientBuilder::beta`](crate::ClientBuilder::beta) (see
//!   [`BetaHeader::TaskBudgets`](crate::BetaHeader::TaskBudgets)).
//!
//! ```no_run
//! use claude_api::messages::{CreateMessageRequest, OutputFormat};
//! use claude_api::types::ModelId;
//! use serde_json::json;
//!
//! # fn main() -> Result<(), claude_api::Error> {
//! let req = CreateMessageRequest::builder()
//!     .model(ModelId::SONNET_4_6)
//!     .max_tokens(1024)
//!     .user("Extract the person's name and age from: Ada Lovelace, 36.")
//!     .output_format(OutputFormat::json_schema(json!({
//!         "type": "object",
//!         "properties": {
//!             "name": { "type": "string" },
//!             "age": { "type": "integer" }
//!         },
//!         "required": ["name", "age"],
//!         "additionalProperties": false
//!     })))
//!     .build()?;
//! # Ok(()) }
//! ```
//!
//! The schema-constrained JSON is returned in a normal `text` content block;
//! parse it with `serde_json::from_str`. Structured outputs cannot be combined
//! with citations or assistant prefill (the API returns 400).

use serde::{Deserialize, Serialize};

/// Generation-shaping options sent as the request's `output_config`.
///
/// Construct with [`OutputConfig::new`] and the `with_*` methods, or set the
/// fields individually through the request builder
/// ([`output_format`](crate::messages::CreateMessageRequestBuilder::output_format),
/// [`effort`](crate::messages::CreateMessageRequestBuilder::effort),
/// [`task_budget`](crate::messages::CreateMessageRequestBuilder::task_budget)).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OutputConfig {
    /// Constrain the model's output to a schema (structured outputs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<OutputFormat>,
    /// Reasoning effort level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// Advisory token budget for an agentic loop (beta).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_budget: Option<TaskBudget>,
}

impl OutputConfig {
    /// An empty config. Equivalent to [`OutputConfig::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the output [`format`](Self::format).
    #[must_use]
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Set the reasoning [`effort`](Self::effort).
    #[must_use]
    pub fn with_effort(mut self, effort: Effort) -> Self {
        self.effort = Some(effort);
        self
    }

    /// Set the [`task_budget`](Self::task_budget).
    #[must_use]
    pub fn with_task_budget(mut self, task_budget: TaskBudget) -> Self {
        self.task_budget = Some(task_budget);
        self
    }
}

/// How the model's output should be formatted.
///
/// The API currently supports a single variant, `json_schema`, which
/// constrains the response to valid JSON matching the supplied schema
/// (structured outputs). Modeled as an enum so future format types round-trip
/// without a breaking change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum OutputFormat {
    /// Constrain output to JSON matching `schema`.
    JsonSchema {
        /// The JSON Schema the output must satisfy. See the Anthropic
        /// structured-outputs docs for the supported schema subset
        /// (`additionalProperties: false` is required on every object).
        schema: serde_json::Value,
    },
}

impl OutputFormat {
    /// Construct a [`OutputFormat::JsonSchema`] from a raw JSON Schema value.
    #[must_use]
    pub fn json_schema(schema: serde_json::Value) -> Self {
        Self::JsonSchema { schema }
    }

    /// Construct a [`OutputFormat::JsonSchema`] whose schema is derived from a
    /// Rust type via [`schemars`].
    ///
    /// # Panics
    ///
    /// Panics only if the generated `RootSchema` fails to JSON-serialize,
    /// which `schemars` guarantees not to happen for any type that implements
    /// [`schemars::JsonSchema`].
    ///
    /// ```ignore
    /// # use claude_api::messages::OutputFormat;
    /// #[derive(schemars::JsonSchema)]
    /// struct Person { name: String, age: u32 }
    ///
    /// let format = OutputFormat::from_schemars::<Person>();
    /// ```
    #[cfg(feature = "schemars-tools")]
    #[cfg_attr(docsrs, doc(cfg(feature = "schemars-tools")))]
    #[must_use]
    pub fn from_schemars<T: schemars::JsonSchema>() -> Self {
        let schema = schemars::r#gen::SchemaGenerator::default().into_root_schema_for::<T>();
        let schema = serde_json::to_value(schema).expect("RootSchema is always JSON-serializable");
        Self::JsonSchema { schema }
    }
}

/// Reasoning effort: how hard the model works before producing output.
///
/// Higher levels spend more on reasoning. Defaults to [`Effort::High`] when
/// omitted. [`Effort::XHigh`] is only accepted by some models (e.g. Fable 5,
/// Opus 4.7+).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Effort {
    /// Minimal reasoning.
    Low,
    /// Moderate reasoning.
    Medium,
    /// Thorough reasoning (the default when `effort` is omitted).
    High,
    /// Extra-high reasoning. Only accepted by some models.
    XHigh,
    /// Maximum reasoning.
    Max,
}

/// Advisory token budget for an agentic loop (beta).
///
/// Requires the `task-budgets-2026-03-13` beta header (see
/// [`BetaHeader::TaskBudgets`](crate::BetaHeader::TaskBudgets)). The budget is
/// a soft hint spanning the whole loop, not a hard per-request cap --
/// `max_tokens` still bounds each individual response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TaskBudget {
    /// A budget measured in tokens.
    Tokens {
        /// Total token budget for the loop. The API requires at least 20,000.
        total: u64,
        /// Budget carried over from a prior request (used during compaction).
        /// Defaults to `total` when omitted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remaining: Option<u64>,
    },
}

impl TaskBudget {
    /// A token budget with no carried-over `remaining`.
    #[must_use]
    pub fn tokens(total: u64) -> Self {
        Self::Tokens {
            total,
            remaining: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn json_schema_format_round_trips() {
        let f = OutputFormat::json_schema(json!({"type": "object"}));
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(
            v,
            json!({"type": "json_schema", "schema": {"type": "object"}})
        );
        let parsed: OutputFormat = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, f);
    }

    #[test]
    fn effort_serializes_lowercase() {
        for (variant, wire) in [
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "high"),
            (Effort::XHigh, "xhigh"),
            (Effort::Max, "max"),
        ] {
            let v = serde_json::to_value(variant).unwrap();
            assert_eq!(v, json!(wire));
            let parsed: Effort = serde_json::from_value(v).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn task_budget_round_trips() {
        let b = TaskBudget::tokens(20_000);
        assert_eq!(
            serde_json::to_value(b).unwrap(),
            json!({"type": "tokens", "total": 20_000})
        );

        let with_remaining = TaskBudget::Tokens {
            total: 64_000,
            remaining: Some(1_000),
        };
        let v = serde_json::to_value(with_remaining).unwrap();
        assert_eq!(
            v,
            json!({"type": "tokens", "total": 64_000, "remaining": 1_000})
        );
        let parsed: TaskBudget = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, with_remaining);
    }

    #[test]
    fn empty_output_config_serializes_to_empty_object() {
        assert_eq!(
            serde_json::to_value(OutputConfig::new()).unwrap(),
            json!({})
        );
    }

    #[test]
    fn full_output_config_round_trips() {
        let cfg = OutputConfig::new()
            .with_format(OutputFormat::json_schema(json!({"type": "object"})))
            .with_effort(Effort::High)
            .with_task_budget(TaskBudget::tokens(20_000));
        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(
            v,
            json!({
                "format": {"type": "json_schema", "schema": {"type": "object"}},
                "effort": "high",
                "task_budget": {"type": "tokens", "total": 20_000}
            })
        );
        let parsed: OutputConfig = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[cfg(feature = "schemars-tools")]
    #[test]
    fn from_schemars_builds_json_schema() {
        #[derive(schemars::JsonSchema)]
        #[allow(dead_code)]
        struct Person {
            name: String,
            age: u32,
        }
        let OutputFormat::JsonSchema { schema } = OutputFormat::from_schemars::<Person>();
        assert!(schema.is_object());
        assert_eq!(schema["type"], json!("object"));
    }
}
