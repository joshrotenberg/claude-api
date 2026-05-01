//! The Models API.
//!
//! Discover what models are available to your API key, with their
//! capability matrix and per-model token limits.
//!
//! # Endpoints
//!
//! | Method | Path | Function |
//! |---|---|---|
//! | `GET` | `/v1/models` | [`Models::list`] (paginated) |
//! | `GET` | `/v1/models` | [`Models::list_all`] (auto-paginates) |
//! | `GET` | `/v1/models/{id}` | [`Models::get`] |
//!
//! # Quick start
//!
//! ```no_run
//! use claude_api::{Client, models::ListModelsParams};
//! # async fn run() -> Result<(), claude_api::Error> {
//! let client = Client::new("sk-ant-...");
//!
//! // Iterate the full set transparently:
//! for model in client.models().list_all().await? {
//!     println!("{}: {}", model.id.as_str(), model.display_name);
//! }
//!
//! // Or fetch one by ID:
//! let m = client.models().get("claude-sonnet-4-6").await?;
//! println!("{} (max input: {:?})", m.display_name, m.max_input_tokens);
//! # Ok(()) }
//! ```

use serde::{Deserialize, Serialize};

use crate::types::ModelId;

/// Metadata for a single model returned by the Models API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelInfo {
    /// Stable model identifier (e.g. `claude-opus-4-7`).
    pub id: ModelId,
    /// Human-readable display name.
    #[serde(default)]
    pub display_name: String,
    /// Creation timestamp (ISO-8601 string).
    #[serde(default)]
    pub created_at: String,
    /// Wire `type` discriminant; always `"model"`.
    #[serde(rename = "type", default = "default_model_kind")]
    pub kind: String,
    /// Maximum total tokens (input + output) the model can produce in
    /// a single response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Maximum input tokens the model can accept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    /// Capability matrix: which features (citations, code execution,
    /// thinking, image input, etc.) the model supports and at what
    /// level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
}

fn default_model_kind() -> String {
    "model".to_owned()
}

/// Whether a single capability is supported by the model.
///
/// The atomic unit of [`ModelCapabilities`]. New capability flags
/// added by Anthropic show up as new fields on the wider struct;
/// each is shaped as a `CapabilitySupport` so the whole tree
/// destructures uniformly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CapabilitySupport {
    /// `true` if the model supports the capability this entry
    /// describes.
    pub supported: bool,
}

/// Per-model feature matrix returned on every [`ModelInfo`].
///
/// Each leaf is a [`CapabilitySupport`] (a single boolean). The
/// nested capabilities ([`ContextManagementCapability`],
/// [`EffortCapability`], [`ThinkingCapability`]) carry both a
/// top-level `supported` flag and a per-variant breakdown.
///
/// `#[non_exhaustive]` -- new capability fields appear over time;
/// callers should pattern-match conservatively.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelCapabilities {
    /// Batch API support.
    pub batch: CapabilitySupport,
    /// Citation generation in responses.
    pub citations: CapabilitySupport,
    /// Code-execution server tool.
    pub code_execution: CapabilitySupport,
    /// Context-management strategies (`compact`, `clear_thinking`,
    /// etc.).
    pub context_management: ContextManagementCapability,
    /// `effort` (`reasoning_effort`) levels.
    pub effort: EffortCapability,
    /// Image content blocks on requests.
    pub image_input: CapabilitySupport,
    /// PDF document blocks on requests.
    pub pdf_input: CapabilitySupport,
    /// Structured-output / strict-schema mode.
    pub structured_outputs: CapabilitySupport,
    /// Extended thinking (`thinking` block) and its type variants.
    pub thinking: ThinkingCapability,
}

/// Context-management support and strategies.
///
/// `supported` is the top-level gate. The named-strategy fields
/// (one per dated strategy ID) are `Option<CapabilitySupport>` since
/// not every model exposes every strategy and Anthropic ships new
/// strategies with date-suffixed names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ContextManagementCapability {
    /// Whether any context-management strategy is supported.
    pub supported: bool,
    /// `clear_thinking_20251015` strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_thinking_20251015: Option<CapabilitySupport>,
    /// `clear_tool_uses_20250919` strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_tool_uses_20250919: Option<CapabilitySupport>,
    /// `compact_20260112` strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_20260112: Option<CapabilitySupport>,
}

/// `effort` (`reasoning_effort`) capability + per-level support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EffortCapability {
    /// Whether `effort` is supported at all.
    pub supported: bool,
    /// `low` effort.
    pub low: CapabilitySupport,
    /// `medium` effort.
    pub medium: CapabilitySupport,
    /// `high` effort.
    pub high: CapabilitySupport,
    /// `max` effort.
    pub max: CapabilitySupport,
    /// `xhigh` effort (only on some models). Optional in the spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xhigh: Option<CapabilitySupport>,
}

/// Extended-thinking capability + type variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ThinkingCapability {
    /// Whether thinking is supported at all.
    pub supported: bool,
    /// Per-`type` thinking-mode breakdown.
    #[serde(default)]
    pub types: ThinkingTypes,
}

/// Thinking-mode variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ThinkingTypes {
    /// `type: "adaptive"` (auto-decide thinking).
    pub adaptive: CapabilitySupport,
    /// `type: "enabled"` (always think).
    pub enabled: CapabilitySupport,
}

/// Query parameters for `GET /v1/models`.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct ListModelsParams {
    /// Cursor for backward pagination: page items before this `id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    /// Cursor for forward pagination: page items after this `id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
    /// Page size (server-defaulted if omitted; 1..=1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl ListModelsParams {
    /// Set the `after_id` cursor (forward paging).
    #[must_use]
    pub fn after_id(mut self, id: impl Into<String>) -> Self {
        self.after_id = Some(id.into());
        self
    }

    /// Set the `before_id` cursor (backward paging).
    #[must_use]
    pub fn before_id(mut self, id: impl Into<String>) -> Self {
        self.before_id = Some(id.into());
        self
    }

    /// Set the page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub use api::Models;

#[cfg(feature = "async")]
mod api {
    use super::{ListModelsParams, ModelInfo};
    use crate::client::Client;
    use crate::error::Result;
    use crate::pagination::Paginated;

    /// Namespace handle for the Models API.
    ///
    /// Obtained via [`Client::models`](crate::Client::models).
    pub struct Models<'a> {
        client: &'a Client,
    }

    impl<'a> Models<'a> {
        pub(crate) fn new(client: &'a Client) -> Self {
            Self { client }
        }

        /// Fetch one page of models.
        pub async fn list(&self, params: ListModelsParams) -> Result<Paginated<ModelInfo>> {
            let params_ref = &params;
            self.client
                .execute_with_retry(
                    || {
                        self.client
                            .request_builder(reqwest::Method::GET, "/v1/models")
                            .query(params_ref)
                    },
                    &[],
                )
                .await
        }

        /// Fetch all models, transparently paging until exhausted.
        ///
        /// Returns the full list as a single `Vec`. Use [`Self::list`] if
        /// you need to control paging yourself (e.g. for backpressure).
        pub async fn list_all(&self) -> Result<Vec<ModelInfo>> {
            let mut all = Vec::new();
            let mut params = ListModelsParams::default();
            loop {
                let page = self.list(params.clone()).await?;
                let next_cursor = page.next_after().map(str::to_owned);
                all.extend(page.data);
                match next_cursor {
                    Some(cursor) => params.after_id = Some(cursor),
                    None => break,
                }
            }
            Ok(all)
        }

        /// Fetch metadata for a single model by ID.
        pub async fn get(&self, id: impl AsRef<str>) -> Result<ModelInfo> {
            let path = format!("/v1/models/{}", id.as_ref());
            self.client
                .execute_with_retry(
                    || self.client.request_builder(reqwest::Method::GET, &path),
                    &[],
                )
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn model_info_round_trips_with_known_fields() {
        let raw = json!({
            "type": "model",
            "id": "claude-opus-4-7",
            "display_name": "Claude Opus 4.7",
            "created_at": "2025-12-01T00:00:00Z"
        });
        let m: ModelInfo = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(m.id, ModelId::OPUS_4_7);
        assert_eq!(m.display_name, "Claude Opus 4.7");
        assert_eq!(m.created_at, "2025-12-01T00:00:00Z");
        assert_eq!(m.kind, "model");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, raw);
    }

    #[test]
    fn model_info_kind_defaults_to_model_when_missing() {
        let raw = json!({"id": "claude-x", "display_name": "X", "created_at": "2025"});
        let m: ModelInfo = serde_json::from_value(raw).unwrap();
        assert_eq!(m.kind, "model");
    }

    #[test]
    fn list_models_params_default_serializes_to_empty_object() {
        let p = ListModelsParams::default();
        assert_eq!(serde_json::to_value(&p).unwrap(), json!({}));
    }

    #[test]
    fn list_models_params_builder_methods() {
        let p = ListModelsParams::default().after_id("abc").limit(50);
        assert_eq!(p.after_id.as_deref(), Some("abc"));
        assert_eq!(p.limit, Some(50));
    }
}

#[cfg(all(test, feature = "async"))]
mod api_tests {
    use super::*;
    use crate::client::Client;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn page_body(ids: &[&str], has_more: bool) -> serde_json::Value {
        let data: Vec<_> = ids
            .iter()
            .map(|id| {
                json!({
                    "type": "model",
                    "id": id,
                    "display_name": id,
                    "created_at": "2025-01-01T00:00:00Z"
                })
            })
            .collect();
        json!({
            "data": data,
            "has_more": has_more,
            "first_id": ids.first().unwrap_or(&""),
            "last_id": ids.last().unwrap_or(&"")
        })
    }

    #[tokio::test]
    async fn list_returns_a_single_page() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(page_body(&["claude-opus-4-7", "claude-sonnet-4-6"], false)),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .models()
            .list(ListModelsParams::default())
            .await
            .unwrap();
        assert_eq!(page.data.len(), 2);
        assert_eq!(page.data[0].id, ModelId::OPUS_4_7);
        assert!(!page.has_more);
        assert_eq!(page.next_after(), None);
    }

    #[tokio::test]
    async fn list_passes_pagination_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(query_param("after_id", "claude-x"))
            .and(query_param("limit", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(&[], false)))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let _ = client
            .models()
            .list(ListModelsParams::default().after_id("claude-x").limit(10))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn list_all_pages_through_results_and_concatenates() {
        let mock = MockServer::start().await;
        // First page: has_more = true, last_id = "claude-sonnet-4-6"
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"type": "model", "id": "claude-opus-4-7", "display_name": "O", "created_at": "x"},
                    {"type": "model", "id": "claude-sonnet-4-6", "display_name": "S", "created_at": "x"}
                ],
                "has_more": true,
                "first_id": "claude-opus-4-7",
                "last_id": "claude-sonnet-4-6"
            })))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        // Second page: has_more = false. Wiremock must see after_id=claude-sonnet-4-6.
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(query_param("after_id", "claude-sonnet-4-6"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"type": "model", "id": "claude-haiku-4-5-20251001", "display_name": "H", "created_at": "x"}
                ],
                "has_more": false,
                "first_id": "claude-haiku-4-5-20251001",
                "last_id": "claude-haiku-4-5-20251001"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let all = client.models().list_all().await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, ModelId::OPUS_4_7);
        assert_eq!(all[1].id, ModelId::SONNET_4_6);
        assert_eq!(all[2].id, ModelId::HAIKU_4_5);
    }

    #[tokio::test]
    async fn get_fetches_a_single_model_by_id() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models/claude-opus-4-7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "type": "model",
                "id": "claude-opus-4-7",
                "display_name": "Claude Opus 4.7",
                "created_at": "2025-12-01T00:00:00Z"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let m = client.models().get("claude-opus-4-7").await.unwrap();
        assert_eq!(m.id, ModelId::OPUS_4_7);
        assert_eq!(m.display_name, "Claude Opus 4.7");
    }

    #[tokio::test]
    async fn get_propagates_404_as_api_error() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models/nope"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "type": "error",
                "error": {"type": "not_found_error", "message": "no such model"}
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let err = client.models().get("nope").await.unwrap_err();
        assert_eq!(err.status(), Some(http::StatusCode::NOT_FOUND));
    }

    #[test]
    fn capability_support_round_trips_minimal_payload() {
        let raw = json!({"supported": true});
        let cs: CapabilitySupport = serde_json::from_value(raw.clone()).unwrap();
        assert!(cs.supported);
        assert_eq!(serde_json::to_value(cs).unwrap(), raw);
    }

    #[test]
    fn model_capabilities_decodes_full_real_world_response() {
        // Lifted verbatim from the live cassette
        // (live_models_get_sonnet_4_6.jsonl). Pin against drift.
        let raw = json!({
            "batch": {"supported": true},
            "citations": {"supported": true},
            "code_execution": {"supported": true},
            "context_management": {
                "clear_thinking_20251015": {"supported": true},
                "clear_tool_uses_20250919": {"supported": true},
                "compact_20260112": {"supported": true},
                "supported": true
            },
            "effort": {
                "high": {"supported": true},
                "low": {"supported": true},
                "max": {"supported": true},
                "medium": {"supported": true},
                "supported": true
            },
            "image_input": {"supported": true},
            "pdf_input": {"supported": true},
            "structured_outputs": {"supported": true},
            "thinking": {
                "supported": true,
                "types": {
                    "adaptive": {"supported": true},
                    "enabled": {"supported": true}
                }
            }
        });
        let caps: ModelCapabilities = serde_json::from_value(raw).unwrap();
        assert!(caps.batch.supported);
        assert!(caps.context_management.supported);
        assert_eq!(
            caps.context_management
                .clear_thinking_20251015
                .map(|c| c.supported),
            Some(true),
        );
        assert!(caps.effort.high.supported);
        assert!(caps.effort.xhigh.is_none(), "xhigh absent on this model");
        assert!(caps.thinking.types.adaptive.supported);
    }

    #[test]
    fn model_capabilities_tolerates_optional_strategy_fields_missing() {
        // A model that doesn't expose the dated context-management
        // strategy fields should still decode.
        let raw = json!({
            "batch": {"supported": false},
            "citations": {"supported": false},
            "code_execution": {"supported": false},
            "context_management": {"supported": false},
            "effort": {
                "high": {"supported": false},
                "low": {"supported": false},
                "max": {"supported": false},
                "medium": {"supported": false},
                "supported": false
            },
            "image_input": {"supported": false},
            "pdf_input": {"supported": false},
            "structured_outputs": {"supported": false},
            "thinking": {
                "supported": false,
                "types": {
                    "adaptive": {"supported": false},
                    "enabled": {"supported": false}
                }
            }
        });
        let caps: ModelCapabilities = serde_json::from_value(raw).unwrap();
        assert!(caps.context_management.clear_thinking_20251015.is_none());
        assert!(caps.context_management.clear_tool_uses_20250919.is_none());
        assert!(caps.context_management.compact_20260112.is_none());
    }

    #[test]
    fn effort_capability_decodes_xhigh_when_present() {
        let raw = json!({
            "supported": true,
            "low": {"supported": true},
            "medium": {"supported": true},
            "high": {"supported": true},
            "max": {"supported": true},
            "xhigh": {"supported": true}
        });
        let e: EffortCapability = serde_json::from_value(raw).unwrap();
        assert_eq!(e.xhigh.map(|c| c.supported), Some(true));
    }

    #[test]
    fn model_info_with_capabilities_round_trips() {
        let raw = json!({
            "type": "model",
            "id": "claude-sonnet-4-6",
            "display_name": "Claude Sonnet 4.6",
            "created_at": "2025-09-29T00:00:00Z",
            "max_tokens": 64_000,
            "max_input_tokens": 200_000,
            "capabilities": {
                "batch": {"supported": true},
                "citations": {"supported": true},
                "code_execution": {"supported": true},
                "context_management": {"supported": true},
                "effort": {
                    "high": {"supported": true},
                    "low": {"supported": true},
                    "max": {"supported": true},
                    "medium": {"supported": true},
                    "supported": true
                },
                "image_input": {"supported": true},
                "pdf_input": {"supported": true},
                "structured_outputs": {"supported": true},
                "thinking": {
                    "supported": true,
                    "types": {
                        "adaptive": {"supported": true},
                        "enabled": {"supported": true}
                    }
                }
            }
        });
        let m: ModelInfo = serde_json::from_value(raw).unwrap();
        let caps = m.capabilities.unwrap();
        assert!(caps.thinking.supported);
        assert_eq!(m.max_tokens, Some(64_000));
    }
}
