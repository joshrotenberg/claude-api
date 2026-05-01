//! Usage reports: messages + `claude_code`.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;

// =====================================================================
// Common dimensions / filters
// =====================================================================

/// Time-bucket granularity for the messages usage report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum BucketWidth {
    /// One-minute buckets.
    #[serde(rename = "1m")]
    Minute,
    /// One-hour buckets.
    #[serde(rename = "1h")]
    Hour,
    /// One-day buckets.
    #[serde(rename = "1d")]
    Day,
}

/// Service-tier categories used in the messages usage report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ServiceTier {
    /// Standard.
    Standard,
    /// Batch.
    Batch,
    /// Priority.
    Priority,
    /// On-demand priority.
    PriorityOnDemand,
    /// Flex.
    Flex,
    /// Discounted flex.
    FlexDiscount,
}

/// Context-window classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ContextWindow {
    /// 0-200k tokens.
    #[serde(rename = "0-200k")]
    Up200k,
    /// 200k-1M tokens.
    #[serde(rename = "200k-1M")]
    Up1M,
}

/// Inference geo classification used in usage rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InferenceGeo {
    /// Global routing.
    Global,
    /// US-only.
    Us,
    /// Model doesn't expose `inference_geo`.
    NotAvailable,
}

/// Group-by dimensions for the messages usage report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MessagesGroupBy {
    /// Group by API key ID.
    ApiKeyId,
    /// Group by workspace ID.
    WorkspaceId,
    /// Group by model.
    Model,
    /// Group by service tier.
    ServiceTier,
    /// Group by context window bucket.
    ContextWindow,
    /// Group by inference geo.
    InferenceGeo,
    /// Group by speed (`fast-mode-2026-02-01` beta).
    Speed,
    /// Group by user account.
    AccountId,
    /// Group by service account.
    ServiceAccountId,
}

/// Inference speed (for `group_by=speed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Speed {
    /// Default speed.
    Standard,
    /// Premium-priced fast inference.
    Fast,
}

// =====================================================================
// Messages usage report
// =====================================================================

/// Cache-creation token breakdown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CacheCreationTokens {
    /// Tokens used to create 5-minute cache entries.
    pub ephemeral_5m_input_tokens: u64,
    /// Tokens used to create 1-hour cache entries.
    pub ephemeral_1h_input_tokens: u64,
}

/// Server-side tool-use breakdown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ServerToolUse {
    /// Web-search request count.
    pub web_search_requests: u64,
}

/// One usage row inside a messages usage report bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessagesUsageRow {
    /// Cumulative cache-creation tokens.
    pub cache_creation: CacheCreationTokens,
    /// Cache-read input tokens.
    pub cache_read_input_tokens: u64,
    /// Uncached input tokens.
    pub uncached_input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Server-tool counts.
    pub server_tool_use: ServerToolUse,

    /// Account dimension (set when grouping by `account_id`).
    #[serde(default)]
    pub account_id: Option<String>,
    /// Service-account dimension.
    #[serde(default)]
    pub service_account_id: Option<String>,
    /// API-key dimension.
    #[serde(default)]
    pub api_key_id: Option<String>,
    /// Workspace dimension.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Model dimension.
    #[serde(default)]
    pub model: Option<String>,
    /// Service-tier dimension.
    #[serde(default)]
    pub service_tier: Option<ServiceTier>,
    /// Context-window dimension.
    #[serde(default)]
    pub context_window: Option<ContextWindow>,
    /// Inference-geo dimension. May be the literal `"not_available"`
    /// for models that don't expose `inference_geo`.
    #[serde(default)]
    pub inference_geo: Option<String>,
}

/// One time bucket in a messages usage report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessagesUsageBucket {
    /// Inclusive start (RFC3339).
    pub starting_at: String,
    /// Exclusive end (RFC3339).
    pub ending_at: String,
    /// Per-row breakdown for the bucket.
    pub results: Vec<MessagesUsageRow>,
}

/// Response shape for `GET /v1/organizations/usage_report/messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessagesUsageReport {
    /// Time-bucket data.
    pub data: Vec<MessagesUsageBucket>,
    /// Whether more pages exist.
    #[serde(default)]
    pub has_more: bool,
    /// Opaque cursor for the next page.
    #[serde(default)]
    pub next_page: Option<String>,
}

/// Filters for [`UsageReport::messages`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct MessagesUsageParams {
    /// RFC3339 start (inclusive). Required.
    pub starting_at: String,
    /// RFC3339 end (exclusive).
    pub ending_at: Option<String>,
    /// Bucket width.
    pub bucket_width: Option<BucketWidth>,
    /// Restrict to specific account IDs.
    pub account_ids: Vec<String>,
    /// Restrict to specific API key IDs.
    pub api_key_ids: Vec<String>,
    /// Restrict to specific service-account IDs.
    pub service_account_ids: Vec<String>,
    /// Restrict to specific workspace IDs.
    pub workspace_ids: Vec<String>,
    /// Restrict to specific models.
    pub models: Vec<String>,
    /// Restrict to specific context windows.
    pub context_window: Vec<ContextWindow>,
    /// Restrict to specific service tiers.
    pub service_tiers: Vec<ServiceTier>,
    /// Restrict to specific inference geos.
    pub inference_geos: Vec<InferenceGeo>,
    /// Restrict to specific speeds (research preview, requires
    /// `fast-mode-2026-02-01` beta).
    pub speeds: Vec<Speed>,
    /// Group rows by these dimensions.
    pub group_by: Vec<MessagesGroupBy>,
    /// Maximum buckets per page.
    pub limit: Option<u32>,
    /// Pagination cursor.
    pub page: Option<String>,
}

impl MessagesUsageParams {
    /// Build a new params with the required `starting_at` field.
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
        if let Some(b) = self.bucket_width {
            q.push((
                "bucket_width",
                match b {
                    BucketWidth::Minute => "1m".into(),
                    BucketWidth::Hour => "1h".into(),
                    BucketWidth::Day => "1d".into(),
                },
            ));
        }
        for v in &self.account_ids {
            q.push(("account_ids[]", v.clone()));
        }
        for v in &self.api_key_ids {
            q.push(("api_key_ids[]", v.clone()));
        }
        for v in &self.service_account_ids {
            q.push(("service_account_ids[]", v.clone()));
        }
        for v in &self.workspace_ids {
            q.push(("workspace_ids[]", v.clone()));
        }
        for v in &self.models {
            q.push(("models[]", v.clone()));
        }
        for v in &self.context_window {
            q.push((
                "context_window[]",
                match v {
                    ContextWindow::Up200k => "0-200k".into(),
                    ContextWindow::Up1M => "200k-1M".into(),
                },
            ));
        }
        for v in &self.service_tiers {
            let s = match v {
                ServiceTier::Standard => "standard",
                ServiceTier::Batch => "batch",
                ServiceTier::Priority => "priority",
                ServiceTier::PriorityOnDemand => "priority_on_demand",
                ServiceTier::Flex => "flex",
                ServiceTier::FlexDiscount => "flex_discount",
            };
            q.push(("service_tiers[]", s.into()));
        }
        for v in &self.inference_geos {
            let s = match v {
                InferenceGeo::Global => "global",
                InferenceGeo::Us => "us",
                InferenceGeo::NotAvailable => "not_available",
            };
            q.push(("inference_geos[]", s.into()));
        }
        for v in &self.speeds {
            q.push((
                "speeds[]",
                match v {
                    Speed::Standard => "standard".into(),
                    Speed::Fast => "fast".into(),
                },
            ));
        }
        for v in &self.group_by {
            let s = match v {
                MessagesGroupBy::ApiKeyId => "api_key_id",
                MessagesGroupBy::WorkspaceId => "workspace_id",
                MessagesGroupBy::Model => "model",
                MessagesGroupBy::ServiceTier => "service_tier",
                MessagesGroupBy::ContextWindow => "context_window",
                MessagesGroupBy::InferenceGeo => "inference_geo",
                MessagesGroupBy::Speed => "speed",
                MessagesGroupBy::AccountId => "account_id",
                MessagesGroupBy::ServiceAccountId => "service_account_id",
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

// =====================================================================
// Claude Code usage report
// =====================================================================

/// Actor on a [`ClaudeCodeRow`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ClaudeCodeActor {
    /// User actor.
    UserActor {
        /// Email of the user.
        email_address: String,
    },
    /// API-key actor.
    ApiActor {
        /// Name of the API key.
        api_key_name: String,
    },
}

/// Lines-of-code statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LinesOfCode {
    /// Lines added.
    pub added: u64,
    /// Lines removed.
    pub removed: u64,
}

/// Per-actor productivity metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClaudeCodeCoreMetrics {
    /// Distinct sessions.
    pub num_sessions: u64,
    /// Lines added/removed.
    pub lines_of_code: LinesOfCode,
    /// Commits authored via Claude Code's commit feature.
    pub commits_by_claude_code: u64,
    /// PRs authored via Claude Code's PR feature.
    pub pull_requests_by_claude_code: u64,
}

/// Estimated cost amount.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CostAmount {
    /// Amount in minor currency units (e.g. cents).
    pub amount: f64,
    /// Currency code (`"USD"`).
    pub currency: String,
}

/// Per-model token usage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClaudeCodeTokens {
    /// Cache-creation tokens.
    pub cache_creation: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
}

/// Per-model breakdown row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClaudeCodeModelBreakdown {
    /// Model name.
    pub model: String,
    /// Token usage.
    pub tokens: ClaudeCodeTokens,
    /// Estimated cost.
    pub estimated_cost: CostAmount,
}

/// Tool action accept/reject counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolActionCounts {
    /// Number of accepted proposals.
    pub accepted: u64,
    /// Number of rejected proposals.
    pub rejected: u64,
}

/// Customer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CustomerType {
    /// API customer.
    Api,
    /// Subscription (Pro / Team).
    Subscription,
}

/// Subscription tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SubscriptionType {
    /// Enterprise tier.
    Enterprise,
    /// Team tier.
    Team,
}

/// One row in the Claude Code usage report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClaudeCodeRow {
    /// UTC date in YYYY-MM-DD.
    pub date: String,
    /// Organization ID.
    pub organization_id: String,
    /// Customer type.
    pub customer_type: CustomerType,
    /// Subscription tier (when `customer_type=subscription`).
    #[serde(default)]
    pub subscription_type: Option<SubscriptionType>,
    /// Actor.
    pub actor: ClaudeCodeActor,
    /// Productivity metrics.
    pub core_metrics: ClaudeCodeCoreMetrics,
    /// Per-model breakdown.
    pub model_breakdown: Vec<ClaudeCodeModelBreakdown>,
    /// Terminal type.
    pub terminal_type: String,
    /// Tool-action acceptance map (tool name → counts).
    #[serde(default)]
    pub tool_actions: std::collections::HashMap<String, ToolActionCounts>,
}

/// Response shape for `GET /v1/organizations/usage_report/claude_code`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClaudeCodeUsageReport {
    /// Daily rows.
    pub data: Vec<ClaudeCodeRow>,
    /// Whether more pages exist.
    #[serde(default)]
    pub has_more: bool,
    /// Opaque next-page cursor.
    #[serde(default)]
    pub next_page: Option<String>,
}

/// Filters for [`UsageReport::claude_code`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ClaudeCodeUsageParams {
    /// UTC date in YYYY-MM-DD. Required (single-day report).
    pub starting_at: String,
    /// Page size (default 20, max 1000).
    pub limit: Option<u32>,
    /// Pagination cursor.
    pub page: Option<String>,
}

impl ClaudeCodeUsageParams {
    /// Build with the required date.
    #[must_use]
    pub fn for_date(date: impl Into<String>) -> Self {
        Self {
            starting_at: date.into(),
            ..Self::default()
        }
    }

    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        q.push(("starting_at", self.starting_at.clone()));
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        q
    }
}

// =====================================================================
// Namespace handle
// =====================================================================

/// Namespace handle for the usage-report endpoints.
pub struct UsageReport<'a> {
    client: &'a Client,
}

impl<'a> UsageReport<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /v1/organizations/usage_report/messages`.
    pub async fn messages(&self, params: MessagesUsageParams) -> Result<MessagesUsageReport> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self.client.request_builder(
                        reqwest::Method::GET,
                        "/v1/organizations/usage_report/messages",
                    );
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/usage_report/claude_code`.
    pub async fn claude_code(
        &self,
        params: ClaudeCodeUsageParams,
    ) -> Result<ClaudeCodeUsageReport> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self.client.request_builder(
                        reqwest::Method::GET,
                        "/v1/organizations/usage_report/claude_code",
                    );
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
    async fn messages_usage_report_decodes_typed_buckets() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/usage_report/messages"))
            .and(wiremock::matchers::query_param(
                "starting_at",
                "2026-05-01T00:00:00Z",
            ))
            .and(wiremock::matchers::query_param("bucket_width", "1d"))
            .and(wiremock::matchers::query_param("group_by[]", "model"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{
                    "starting_at": "2026-05-01T00:00:00Z",
                    "ending_at": "2026-05-02T00:00:00Z",
                    "results": [{
                        "cache_creation": {
                            "ephemeral_5m_input_tokens": 0,
                            "ephemeral_1h_input_tokens": 0
                        },
                        "cache_read_input_tokens": 0,
                        "uncached_input_tokens": 100,
                        "output_tokens": 50,
                        "server_tool_use": {"web_search_requests": 0},
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
            .usage_report()
            .messages(MessagesUsageParams {
                starting_at: "2026-05-01T00:00:00Z".into(),
                bucket_width: Some(BucketWidth::Day),
                group_by: vec![MessagesGroupBy::Model],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(r.data.len(), 1);
        assert_eq!(
            r.data[0].results[0].model.as_deref(),
            Some("claude-sonnet-4-6")
        );
    }

    #[tokio::test]
    async fn claude_code_usage_report_decodes_user_actor_row() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/usage_report/claude_code"))
            .and(wiremock::matchers::query_param("starting_at", "2026-05-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{
                    "date": "2026-05-01",
                    "organization_id": "org_01",
                    "customer_type": "api",
                    "actor": {"type": "user_actor", "email_address": "u@example.com"},
                    "core_metrics": {
                        "num_sessions": 4,
                        "lines_of_code": {"added": 100, "removed": 20},
                        "commits_by_claude_code": 2,
                        "pull_requests_by_claude_code": 1
                    },
                    "model_breakdown": [],
                    "terminal_type": "iterm",
                    "tool_actions": {
                        "edit": {"accepted": 3, "rejected": 1}
                    }
                }],
                "has_more": false,
                "next_page": null
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let r = client
            .admin()
            .usage_report()
            .claude_code(ClaudeCodeUsageParams::for_date("2026-05-01"))
            .await
            .unwrap();
        assert_eq!(r.data.len(), 1);
        match &r.data[0].actor {
            ClaudeCodeActor::UserActor { email_address } => {
                assert_eq!(email_address, "u@example.com");
            }
            ClaudeCodeActor::ApiActor { .. } => panic!("expected user actor"),
        }
        assert_eq!(r.data[0].core_metrics.num_sessions, 4);
    }
}
