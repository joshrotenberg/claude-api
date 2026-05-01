//! Sessions: provision, retrieve, list, archive, delete.
//!
//! A session is a running agent instance within an environment. Each
//! session references an [agent](super::agents) (by ID or pinned to a
//! version) and an environment, and maintains conversation history
//! across multiple interactions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::agents::{AgentMcpServer, AgentModel, AgentTool, Skill};
use super::resources::SessionResource;
use super::MANAGED_AGENTS_BETA;

/// Session lifecycle status.
///
/// Sessions start in [`Idle`](Self::Idle), transition to
/// [`Running`](Self::Running) while processing, and may briefly
/// [`Rescheduling`](Self::Rescheduling) on transient retries.
/// [`Terminated`](Self::Terminated) is terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionStatus {
    /// Agent is waiting for input. Sessions start in this state.
    Idle,
    /// Agent is actively executing.
    Running,
    /// Transient error occurred; session is retrying automatically.
    Rescheduling,
    /// Session has ended due to an unrecoverable error.
    Terminated,
}

/// Reference to an agent: either a string ID (latest version) or a
/// pinned `{type, id, version}` object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum AgentRef {
    /// Bare ID; resolves to the latest published version of the agent.
    Latest(String),
    /// Pinned to a specific version.
    Pinned {
        /// Always `"agent"`.
        #[serde(rename = "type")]
        ty: String,
        /// Agent ID.
        id: String,
        /// Agent version number.
        version: u32,
    },
}

impl AgentRef {
    /// Build an [`AgentRef::Latest`].
    #[must_use]
    pub fn latest(id: impl Into<String>) -> Self {
        Self::Latest(id.into())
    }

    /// Build an [`AgentRef::Pinned`] for a specific version.
    #[must_use]
    pub fn pinned(id: impl Into<String>, version: u32) -> Self {
        Self::Pinned {
            ty: "agent".into(),
            id: id.into(),
            version,
        }
    }
}

impl From<&str> for AgentRef {
    fn from(s: &str) -> Self {
        Self::latest(s)
    }
}

impl From<String> for AgentRef {
    fn from(s: String) -> Self {
        Self::latest(s)
    }
}

/// Cumulative token usage on a session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionUsage {
    /// Uncached input tokens billed across all model calls.
    #[serde(default)]
    pub input_tokens: u64,
    /// Total output tokens across all model calls.
    #[serde(default)]
    pub output_tokens: u64,
    /// Tokens written to the prompt cache, broken down by cache TTL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation: Option<CacheCreationUsage>,
    /// Tokens served from the prompt cache (cheaper read path).
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

/// Prompt-cache write tokens, split by ephemeral TTL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CacheCreationUsage {
    /// Tokens used to create 5-minute ephemeral cache entries.
    #[serde(default)]
    pub ephemeral_5m_input_tokens: u64,
    /// Tokens used to create 1-hour ephemeral cache entries.
    #[serde(default)]
    pub ephemeral_1h_input_tokens: u64,
}

/// A Managed Agents session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Session {
    /// Stable session identifier (`sesn_...`).
    pub id: String,
    /// Wire `type`; always `"session"`.
    #[serde(rename = "type", default = "default_session_kind")]
    pub kind: String,
    /// Lifecycle status.
    pub status: SessionStatus,
    /// Resolved snapshot of the agent at session-creation time. Pinned
    /// even if the underlying agent is later updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<SessionAgent>,
    /// ID of the environment this session runs in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Vault references for MCP credential resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vault_ids: Vec<String>,
    /// Optional human-readable title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Free-form key-value metadata attached at create time.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Cumulative token usage. May be absent on freshly-created sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<SessionUsage>,
    /// Wall-clock and active-runtime stats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<SessionStats>,
    /// Mounted resources: file uploads, GitHub repositories, memory
    /// stores. Each carries a server-assigned `id` (`sesrsc_...`) used
    /// for [`Resources::update`](super::resources::Resources::update)
    /// and [`Resources::delete`](super::resources::Resources::delete).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<SessionResource>,
    /// Outcome evaluations recorded against this session, when an
    /// outcome was defined. **Research-preview**: this field is
    /// populated only when the session uses the outcomes feature
    /// (`user.define_outcome` events, requires
    /// `managed-agents-2026-04-01-research-preview` beta header).
    /// Preserved as `Vec<Value>` until the outcomes types land.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcome_evaluations: Vec<serde_json::Value>,
    /// Timestamp when the session was created (RFC3339 / ISO 8601).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Timestamp of the most recent update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Set when the session has been archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

fn default_session_kind() -> String {
    "session".to_owned()
}

/// Snapshot of an agent's configuration at the moment a session was
/// created. Mirrors [`Agent`](super::agents::Agent) but is pinned --
/// later edits to the agent don't change a session's recorded
/// snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionAgent {
    /// Wire `type`; always `"agent"`.
    #[serde(rename = "type", default = "default_session_agent_kind")]
    pub kind: String,
    /// Agent ID (`agnt_...`).
    pub id: String,
    /// Pinned agent version.
    pub version: u32,
    /// Agent name as it was at snapshot time.
    pub name: String,
    /// Agent description. May be `null` if no description was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Model configuration.
    pub model: AgentModel,
    /// System prompt. May be `null` if no system prompt was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Tools available to the agent at snapshot time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AgentTool>,
    /// MCP servers configured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<AgentMcpServer>,
    /// Skills loaded into the container.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<Skill>,
}

fn default_session_agent_kind() -> String {
    "agent".to_owned()
}

/// Wall-clock and active-runtime statistics for a session.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionStats {
    /// Total elapsed seconds since session creation.
    pub duration_seconds: f64,
    /// Seconds the agent was actively executing (excluding idle time).
    pub active_seconds: f64,
}

/// Request body for `POST /v1/sessions`.
///
/// Build via [`CreateSessionRequest::builder`].
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateSessionRequest {
    /// The agent driving this session. May be a bare ID string (latest
    /// version) or a pinned [`AgentRef::Pinned`].
    pub agent: AgentRef,
    /// Environment the session runs in.
    pub environment_id: String,
    /// Optional vault references for MCP credential resolution.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vault_ids: Vec<String>,
    /// Optional resources mounted into the session container at
    /// creation time. Build with the typed constructors in
    /// [`crate::managed_agents::resources`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<SessionResource>,
    /// Optional human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl CreateSessionRequest {
    /// Begin configuring a request.
    #[must_use]
    pub fn builder() -> CreateSessionRequestBuilder {
        CreateSessionRequestBuilder::default()
    }
}

/// Builder for [`CreateSessionRequest`].
#[derive(Debug, Default)]
pub struct CreateSessionRequestBuilder {
    agent: Option<AgentRef>,
    environment_id: Option<String>,
    vault_ids: Vec<String>,
    resources: Vec<SessionResource>,
    title: Option<String>,
}

impl CreateSessionRequestBuilder {
    /// Set the agent. Required.
    #[must_use]
    pub fn agent(mut self, agent: impl Into<AgentRef>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    /// Set the environment. Required.
    #[must_use]
    pub fn environment_id(mut self, id: impl Into<String>) -> Self {
        self.environment_id = Some(id.into());
        self
    }

    /// Append a vault ID for credential resolution.
    #[must_use]
    pub fn vault_id(mut self, id: impl Into<String>) -> Self {
        self.vault_ids.push(id.into());
        self
    }

    /// Set the full vault list.
    #[must_use]
    pub fn vault_ids(mut self, ids: Vec<String>) -> Self {
        self.vault_ids = ids;
        self
    }

    /// Append a typed resource (file / `github_repository` /
    /// `memory_store`). Build via the constructors in
    /// [`crate::managed_agents::resources`].
    #[must_use]
    pub fn resource(mut self, resource: SessionResource) -> Self {
        self.resources.push(resource);
        self
    }

    /// Set a human-readable title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Finalize.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`](crate::Error::InvalidConfig)
    /// if `agent` or `environment_id` was not set.
    pub fn build(self) -> Result<CreateSessionRequest> {
        let agent = self
            .agent
            .ok_or_else(|| crate::Error::InvalidConfig("agent is required".into()))?;
        let environment_id = self
            .environment_id
            .ok_or_else(|| crate::Error::InvalidConfig("environment_id is required".into()))?;
        Ok(CreateSessionRequest {
            agent,
            environment_id,
            vault_ids: self.vault_ids,
            resources: self.resources,
            title: self.title,
        })
    }
}

/// Optional knobs for [`Sessions::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListSessionsParams {
    /// Pagination cursor: results after this session ID.
    pub after: Option<String>,
    /// Pagination cursor: results before this session ID.
    pub before: Option<String>,
    /// Page size limit.
    pub limit: Option<u32>,
    /// Whether to include archived sessions.
    pub include_archived: Option<bool>,
}

impl ListSessionsParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(a) = &self.after {
            q.push(("after", a.clone()));
        }
        if let Some(b) = &self.before {
            q.push(("before", b.clone()));
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(ia) = self.include_archived {
            q.push(("include_archived", ia.to_string()));
        }
        q
    }
}

/// Namespace handle for the Sessions API.
///
/// Obtained via [`Client::managed_agents`](Client::managed_agents).
pub struct Sessions<'a> {
    client: &'a Client,
}

/// Request body for [`Sessions::update`]. All fields optional with
/// merge-patch semantics: omit a field to preserve.
///
/// `metadata` is per-key: provide a `Some(value)` to upsert, or `None`
/// to delete the key. Unspecified keys are preserved.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateSessionRequest {
    /// New human-readable title (1-500 chars). `None` to leave
    /// unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Per-key metadata patch. See [`MetadataPatch`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<super::agents::MetadataPatch>,
    /// Replace the vault attachments. **Currently rejected by the
    /// server**; reserved for future use.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vault_ids: Vec<String>,
}

impl UpdateSessionRequest {
    /// Empty patch.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the new title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Apply a metadata patch.
    #[must_use]
    pub fn metadata(mut self, patch: super::agents::MetadataPatch) -> Self {
        self.metadata = Some(patch);
        self
    }
}

impl<'a> Sessions<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Create a session. Returns the freshly-provisioned [`Session`].
    pub async fn create(&self, request: CreateSessionRequest) -> Result<Session> {
        let request_ref = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/sessions")
                        .json(request_ref)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// Fetch a session by ID.
    pub async fn retrieve(&self, session_id: &str) -> Result<Session> {
        let path = format!("/v1/sessions/{session_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// List sessions, paginated.
    pub async fn list(&self, params: ListSessionsParams) -> Result<Paginated<Session>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/sessions");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// Update a session. All fields on [`UpdateSessionRequest`] are
    /// optional with merge-patch semantics: omit a field to preserve
    /// its current value.
    pub async fn update(&self, session_id: &str, request: UpdateSessionRequest) -> Result<Session> {
        let path = format!("/v1/sessions/{session_id}");
        let request_ref = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(request_ref)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// Archive a session. Archived sessions cannot accept new events but
    /// preserve their event history.
    pub async fn archive(&self, session_id: &str) -> Result<Session> {
        let path = format!("/v1/sessions/{session_id}/archive");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// Sub-namespace for the session-events API
    /// (`/v1/sessions/{id}/events` and the SSE stream).
    #[must_use]
    pub fn events(&self, session_id: impl Into<String>) -> super::events::Events<'_> {
        super::events::Events {
            client: self.client,
            session_id: session_id.into(),
        }
    }

    /// Sub-namespace for resource operations on a session
    /// (`/v1/sessions/{id}/resources`).
    #[must_use]
    pub fn resources(&self, session_id: impl Into<String>) -> super::resources::Resources<'_> {
        super::resources::Resources {
            client: self.client,
            session_id: session_id.into(),
        }
    }

    /// Sub-namespace for thread operations on a multi-agent session
    /// (`/v1/sessions/{id}/threads`). Sub-agent threads are spawned at
    /// runtime when the coordinator delegates to a `callable_agent`.
    #[must_use]
    pub fn threads(&self, session_id: impl Into<String>) -> super::threads::Threads<'_> {
        super::threads::Threads {
            client: self.client,
            session_id: session_id.into(),
        }
    }

    /// Delete a session permanently. The session must not be `running`;
    /// send a `user.interrupt` event first if necessary. Files, memory
    /// stores, environments, and agents are independent and not
    /// affected.
    pub async fn delete(&self, session_id: &str) -> Result<()> {
        let path = format!("/v1/sessions/{session_id}");
        // The delete endpoint returns 204 No Content; route through a
        // dummy `serde_json::Value` to satisfy execute()'s
        // DeserializeOwned bound and discard the result.
        let _: serde_json::Value = self
            .client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn fake_session(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "status": "idle",
            "title": "Test session",
            "usage": {
                "input_tokens": 0,
                "output_tokens": 0,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            },
            "created_at": "2026-04-30T12:00:00Z"
        })
    }

    #[test]
    fn agent_ref_serializes_string_form_untagged() {
        let r = AgentRef::latest("agent_01ABC");
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v, json!("agent_01ABC"));
    }

    #[test]
    fn agent_ref_serializes_pinned_form_with_type_tag() {
        let r = AgentRef::pinned("agent_01ABC", 3);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            json!({"type": "agent", "id": "agent_01ABC", "version": 3})
        );
    }

    #[test]
    fn agent_ref_round_trips_both_forms() {
        for r in [AgentRef::latest("a"), AgentRef::pinned("a", 1)] {
            let v = serde_json::to_value(&r).unwrap();
            let parsed: AgentRef = serde_json::from_value(v).unwrap();
            assert_eq!(parsed, r);
        }
    }

    #[test]
    fn create_session_request_drops_empty_optional_fields() {
        let req = CreateSessionRequest::builder()
            .agent("agent_01")
            .environment_id("env_01")
            .build()
            .unwrap();
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("vault_ids").is_none(), "{v}");
        assert!(v.get("resources").is_none(), "{v}");
        assert!(v.get("title").is_none(), "{v}");
    }

    #[tokio::test]
    async fn create_posts_to_v1_sessions_with_beta_header() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions"))
            .and(header("anthropic-beta", "managed-agents-2026-04-01"))
            .and(body_partial_json(json!({
                "agent": "agent_01",
                "environment_id": "env_01"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_session("sesn_01")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateSessionRequest::builder()
            .agent("agent_01")
            .environment_id("env_01")
            .build()
            .unwrap();
        let s = client
            .managed_agents()
            .sessions()
            .create(req)
            .await
            .unwrap();
        assert_eq!(s.id, "sesn_01");
        assert_eq!(s.status, SessionStatus::Idle);
        assert_eq!(s.title.as_deref(), Some("Test session"));
    }

    #[tokio::test]
    async fn create_with_pinned_agent_serializes_object_form() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions"))
            .and(body_partial_json(json!({
                "agent": {"type": "agent", "id": "agent_01", "version": 2}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_session("sesn_01")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateSessionRequest::builder()
            .agent(AgentRef::pinned("agent_01", 2))
            .environment_id("env_01")
            .build()
            .unwrap();
        let _ = client
            .managed_agents()
            .sessions()
            .create(req)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_with_vault_ids_includes_them_in_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions"))
            .and(body_partial_json(
                json!({"vault_ids": ["vault_01", "vault_02"]}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_session("sesn_01")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateSessionRequest::builder()
            .agent("agent_01")
            .environment_id("env_01")
            .vault_id("vault_01")
            .vault_id("vault_02")
            .build()
            .unwrap();
        let _ = client
            .managed_agents()
            .sessions()
            .create(req)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retrieve_returns_typed_session() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_42"))
            .and(header("anthropic-beta", "managed-agents-2026-04-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sesn_42",
                "status": "running"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let s = client
            .managed_agents()
            .sessions()
            .retrieve("sesn_42")
            .await
            .unwrap();
        assert_eq!(s.id, "sesn_42");
        assert_eq!(s.status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn list_passes_pagination_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions"))
            .and(wiremock::matchers::query_param("limit", "5"))
            .and(wiremock::matchers::query_param("include_archived", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"id": "sesn_a", "status": "idle"},
                    {"id": "sesn_b", "status": "terminated"}
                ],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .sessions()
            .list(ListSessionsParams {
                limit: Some(5),
                include_archived: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 2);
    }

    #[tokio::test]
    async fn archive_posts_to_archive_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions/sesn_x/archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sesn_x",
                "status": "idle",
                "archived_at": "2026-04-30T12:00:00Z"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let s = client
            .managed_agents()
            .sessions()
            .archive("sesn_x")
            .await
            .unwrap();
        assert_eq!(s.archived_at.as_deref(), Some("2026-04-30T12:00:00Z"));
    }

    #[tokio::test]
    async fn delete_returns_unit_on_success() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/sessions/sesn_x"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        client
            .managed_agents()
            .sessions()
            .delete("sesn_x")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn update_posts_to_session_path_with_merge_patch_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions/sesn_u"))
            .and(body_partial_json(json!({
                "title": "renamed",
                "metadata": {"plan": "pro", "old": null}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_session("sesn_u")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let s = client
            .managed_agents()
            .sessions()
            .update(
                "sesn_u",
                UpdateSessionRequest::new().title("renamed").metadata(
                    super::super::agents::MetadataPatch::new()
                        .set("plan", "pro")
                        .delete("old"),
                ),
            )
            .await
            .unwrap();
        assert_eq!(s.id, "sesn_u");
    }

    #[test]
    fn session_decodes_full_response_with_agent_snapshot_environment_and_stats() {
        // Fixture lifted from the spec example for BetaManagedAgentsSession.
        let raw = json!({
            "id": "sesn_full",
            "type": "session",
            "status": "idle",
            "agent": {
                "type": "agent",
                "id": "agent_X",
                "version": 3,
                "name": "Lead",
                "description": "An agent",
                "model": "claude-sonnet-4-6",
                "system": "you are an agent",
                "tools": [],
                "mcp_servers": [],
                "skills": []
            },
            "environment_id": "env_Y",
            "vault_ids": ["vlt_a", "vlt_b"],
            "title": "demo",
            "metadata": {"team": "research"},
            "stats": {"duration_seconds": 123.5, "active_seconds": 45.0},
            "resources": [],
            "created_at": "2026-04-30T12:00:00Z",
            "updated_at": "2026-04-30T12:01:00Z"
        });
        let s: Session = serde_json::from_value(raw).unwrap();
        assert_eq!(s.kind, "session");
        let agent = s.agent.unwrap();
        assert_eq!(agent.id, "agent_X");
        assert_eq!(agent.version, 3);
        assert_eq!(s.environment_id.as_deref(), Some("env_Y"));
        assert_eq!(s.vault_ids, vec!["vlt_a", "vlt_b"]);
        assert_eq!(s.metadata.get("team").map(String::as_str), Some("research"));
        let stats = s.stats.unwrap();
        assert!((stats.duration_seconds - 123.5).abs() < 1e-6);
        assert!((stats.active_seconds - 45.0).abs() < 1e-6);
    }
}
