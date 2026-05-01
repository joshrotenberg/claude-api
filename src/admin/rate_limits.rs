//! Rate-limit listing endpoints (organization-wide and workspace-scoped).

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::PaginatedNextPage;

/// Rate-limit grouping category. Forward-compatible: unknown groups
/// fall through to [`Self::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitGroup {
    /// A model family (per-`model_group`).
    ModelGroup,
    /// Message Batches API.
    Batch,
    /// Token-counting API.
    TokenCount,
    /// Files API.
    Files,
    /// Skills API.
    Skills,
    /// Web search server tool.
    WebSearch,
    /// Unknown group; raw string preserved.
    Other(String),
}

impl Serialize for RateLimitGroup {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            Self::ModelGroup => "model_group",
            Self::Batch => "batch",
            Self::TokenCount => "token_count",
            Self::Files => "files",
            Self::Skills => "skills",
            Self::WebSearch => "web_search",
            Self::Other(v) => v,
        })
    }
}

impl<'de> Deserialize<'de> for RateLimitGroup {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "model_group" => Self::ModelGroup,
            "batch" => Self::Batch,
            "token_count" => Self::TokenCount,
            "files" => Self::Files,
            "skills" => Self::Skills,
            "web_search" => Self::WebSearch,
            _ => Self::Other(s),
        })
    }
}

/// One limiter inside an org-wide [`OrgRateLimitEntry`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OrgLimit {
    /// Limiter type (e.g. `requests_per_minute`, `input_tokens_per_minute`).
    #[serde(rename = "type")]
    pub ty: String,
    /// Configured value.
    pub value: f64,
}

/// One organization-wide rate-limit entry. Returned by
/// [`RateLimits::list_organization`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OrgRateLimitEntry {
    /// Wire type tag (`"rate_limit"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Group category.
    pub group_type: RateLimitGroup,
    /// Models this entry applies to (only when `group_type` is
    /// `model_group`; `null` otherwise).
    #[serde(default)]
    pub models: Option<Vec<String>>,
    /// Per-limiter values.
    pub limits: Vec<OrgLimit>,
}

/// One limiter inside a workspace-level override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WorkspaceLimit {
    /// Limiter type.
    #[serde(rename = "type")]
    pub ty: String,
    /// Workspace override value.
    pub value: f64,
    /// Organization-level value for the same limiter, for reference.
    /// `None` if the org has no limit configured for this limiter.
    #[serde(default)]
    pub org_limit: Option<f64>,
}

/// One workspace-scoped rate-limit entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WorkspaceRateLimitEntry {
    /// Wire type tag (`"workspace_rate_limit"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Group category.
    pub group_type: RateLimitGroup,
    /// Models (when `group_type` is `model_group`).
    #[serde(default)]
    pub models: Option<Vec<String>>,
    /// Per-limiter overrides.
    pub limits: Vec<WorkspaceLimit>,
}

/// Filters for [`RateLimits::list_organization`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListOrgRateLimitsParams {
    /// Filter by group type.
    pub group_type: Option<RateLimitGroup>,
    /// Filter to the entry containing this model (404 if not found).
    pub model: Option<String>,
    /// Pagination cursor.
    pub page: Option<String>,
}

impl ListOrgRateLimitsParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(g) = &self.group_type {
            // Reuse Serialize impl rather than duplicating the match.
            if let Ok(v) = serde_json::to_value(g) {
                if let Some(s) = v.as_str() {
                    q.push(("group_type", s.to_owned()));
                }
            }
        }
        if let Some(m) = &self.model {
            q.push(("model", m.clone()));
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        q
    }
}

/// Filters for [`RateLimits::list_workspace`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListWorkspaceRateLimitsParams {
    /// Filter by group type.
    pub group_type: Option<RateLimitGroup>,
    /// Pagination cursor.
    pub page: Option<String>,
}

impl ListWorkspaceRateLimitsParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(g) = &self.group_type {
            if let Ok(v) = serde_json::to_value(g) {
                if let Some(s) = v.as_str() {
                    q.push(("group_type", s.to_owned()));
                }
            }
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        q
    }
}

/// Namespace handle for rate-limit endpoints.
pub struct RateLimits<'a> {
    client: &'a Client,
}

impl<'a> RateLimits<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /v1/organizations/rate_limits`.
    pub async fn list_organization(
        &self,
        params: ListOrgRateLimitsParams,
    ) -> Result<PaginatedNextPage<OrgRateLimitEntry>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/organizations/rate_limits");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/workspaces/{ws}/rate_limits`.
    pub async fn list_workspace(
        &self,
        workspace_id: &str,
        params: ListWorkspaceRateLimitsParams,
    ) -> Result<PaginatedNextPage<WorkspaceRateLimitEntry>> {
        let path = format!("/v1/organizations/workspaces/{workspace_id}/rate_limits");
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self.client.request_builder(reqwest::Method::GET, &path);
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[],
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-admin-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[test]
    fn rate_limit_group_round_trips_known_and_other_variants() {
        for v in ["model_group", "batch", "files", "skills"] {
            let g: RateLimitGroup = serde_json::from_value(json!(v)).unwrap();
            assert_eq!(serde_json::to_value(&g).unwrap(), json!(v));
        }
        let other: RateLimitGroup = serde_json::from_value(json!("future_group")).unwrap();
        assert_eq!(other, RateLimitGroup::Other("future_group".into()));
    }

    #[tokio::test]
    async fn list_organization_rate_limits_decodes_typed_entries() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/rate_limits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {
                        "type": "rate_limit",
                        "group_type": "model_group",
                        "models": ["claude-opus-4-7"],
                        "limits": [
                            {"type": "requests_per_minute", "value": 1000.0},
                            {"type": "input_tokens_per_minute", "value": 4_000_000.0}
                        ]
                    }
                ],
                "next_page": null
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let r = client
            .admin()
            .rate_limits()
            .list_organization(ListOrgRateLimitsParams::default())
            .await
            .unwrap();
        assert_eq!(r.data.len(), 1);
        assert_eq!(r.data[0].group_type, RateLimitGroup::ModelGroup);
        assert_eq!(r.data[0].limits.len(), 2);
    }

    #[tokio::test]
    async fn list_workspace_rate_limits_returns_overrides_with_org_limit() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/workspaces/ws_01/rate_limits"))
            .and(wiremock::matchers::query_param("group_type", "files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {
                        "type": "workspace_rate_limit",
                        "group_type": "files",
                        "models": null,
                        "limits": [
                            {"type": "requests_per_minute", "value": 100.0, "org_limit": 1000.0}
                        ]
                    }
                ],
                "next_page": null
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let r = client
            .admin()
            .rate_limits()
            .list_workspace(
                "ws_01",
                ListWorkspaceRateLimitsParams {
                    group_type: Some(RateLimitGroup::Files),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(r.data.len(), 1);
        let entry = &r.data[0];
        assert_eq!(entry.group_type, RateLimitGroup::Files);
        assert!(entry.models.is_none());
        assert_eq!(entry.limits[0].org_limit, Some(1000.0));
    }
}
