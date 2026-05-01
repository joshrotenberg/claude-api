//! Cost report.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;

use super::usage_report::{ContextWindow, ServiceTier};

/// Cost-row category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CostType {
    /// Token-based cost.
    Tokens,
    /// Web-search request cost.
    WebSearch,
    /// Code-execution server-tool cost.
    CodeExecution,
    /// Managed Agents session-runtime cost.
    SessionUsage,
}

/// Token sub-type for token costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TokenType {
    /// Uncached input.
    #[serde(rename = "uncached_input_tokens")]
    UncachedInput,
    /// Output.
    #[serde(rename = "output_tokens")]
    Output,
    /// Cache reads.
    #[serde(rename = "cache_read_input_tokens")]
    CacheRead,
    /// Cache creation, 1h TTL.
    #[serde(rename = "cache_creation.ephemeral_1h_input_tokens")]
    CacheCreate1h,
    /// Cache creation, 5m TTL.
    #[serde(rename = "cache_creation.ephemeral_5m_input_tokens")]
    CacheCreate5m,
}

/// Group-by dimensions for the cost report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CostGroupBy {
    /// Group by workspace.
    WorkspaceId,
    /// Group by description (`cost_type` / `token_type` / etc.).
    Description,
}

/// Day-bucket width sentinel (only `1d` is supported on the cost
/// report).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CostBucketWidth {
    /// One day.
    #[serde(rename = "1d")]
    Day,
}

/// One row inside a cost report bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CostRow {
    /// Amount in lowest currency units (e.g. cents) as a decimal
    /// string. `"123.45"` USD = $1.2345.
    pub amount: String,
    /// Currency code (`"USD"`).
    pub currency: String,
    /// Cost category. `None` when not grouping by description.
    #[serde(default)]
    pub cost_type: Option<CostType>,
    /// Token-type subdivision. `None` when not grouping or for
    /// non-token costs.
    #[serde(default)]
    pub token_type: Option<TokenType>,
    /// Description of the cost item. `None` when not grouping by
    /// description.
    #[serde(default)]
    pub description: Option<String>,
    /// Workspace dimension.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Model dimension.
    #[serde(default)]
    pub model: Option<String>,
    /// Context-window dimension.
    #[serde(default)]
    pub context_window: Option<ContextWindow>,
    /// Service-tier dimension.
    #[serde(default)]
    pub service_tier: Option<ServiceTier>,
    /// Inference-geo dimension.
    #[serde(default)]
    pub inference_geo: Option<String>,
}

/// One time bucket in a cost report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CostBucket {
    /// Inclusive start (RFC3339).
    pub starting_at: String,
    /// Exclusive end (RFC3339).
    pub ending_at: String,
    /// Per-row breakdown.
    pub results: Vec<CostRow>,
}

/// Response shape for `GET /v1/organizations/cost_report`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CostReport {
    /// Time-bucket data.
    pub data: Vec<CostBucket>,
    /// Whether more pages exist.
    #[serde(default)]
    pub has_more: bool,
    /// Opaque next-page cursor.
    #[serde(default)]
    pub next_page: Option<String>,
}

/// Filters for [`Cost::report`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct CostReportParams {
    /// RFC3339 start. Required.
    pub starting_at: String,
    /// RFC3339 end.
    pub ending_at: Option<String>,
    /// Bucket width (only `1d`).
    pub bucket_width: Option<CostBucketWidth>,
    /// Group-by dimensions.
    pub group_by: Vec<CostGroupBy>,
    /// Page size.
    pub limit: Option<u32>,
    /// Pagination cursor.
    pub page: Option<String>,
}

impl CostReportParams {
    /// Build with the required `starting_at`.
    #[must_use]
    pub fn starting_at(starting_at: impl Into<String>) -> Self {
        Self {
            starting_at: starting_at.into(),
            ..Self::default()
        }
    }

    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        q.push(("starting_at", self.starting_at.clone()));
        if let Some(e) = &self.ending_at {
            q.push(("ending_at", e.clone()));
        }
        if let Some(_b) = self.bucket_width {
            q.push(("bucket_width", "1d".into()));
        }
        for g in &self.group_by {
            let s = match g {
                CostGroupBy::WorkspaceId => "workspace_id",
                CostGroupBy::Description => "description",
            };
            q.push(("group_by[]", s.into()));
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        q
    }
}

/// Namespace handle for the cost-report endpoint.
pub struct Cost<'a> {
    client: &'a Client,
}

impl<'a> Cost<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /v1/organizations/cost_report`.
    pub async fn report(&self, params: CostReportParams) -> Result<CostReport> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/organizations/cost_report");
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

    #[tokio::test]
    async fn cost_report_groups_by_description_and_decodes_token_type() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .and(wiremock::matchers::query_param(
                "starting_at",
                "2026-05-01T00:00:00Z",
            ))
            .and(wiremock::matchers::query_param("group_by[]", "description"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{
                    "starting_at": "2026-05-01T00:00:00Z",
                    "ending_at": "2026-05-02T00:00:00Z",
                    "results": [{
                        "amount": "123.45",
                        "currency": "USD",
                        "cost_type": "tokens",
                        "token_type": "uncached_input_tokens",
                        "description": "Sonnet 4.6 input",
                        "model": "claude-sonnet-4-6"
                    }]
                }],
                "has_more": false,
                "next_page": null
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let r = client
            .admin()
            .cost()
            .report(CostReportParams {
                starting_at: "2026-05-01T00:00:00Z".into(),
                group_by: vec![CostGroupBy::Description],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(r.data.len(), 1);
        let row = &r.data[0].results[0];
        assert_eq!(row.amount, "123.45");
        assert_eq!(row.cost_type, Some(CostType::Tokens));
        assert_eq!(row.token_type, Some(TokenType::UncachedInput));
    }
}
