//! Memory stores: persistent text documents that survive across sessions.
//!
//! A memory store is a workspace-scoped collection of small text files
//! (memories) addressed by path. Sessions mount stores under
//! `/mnt/memory/` and read/write them with the standard agent toolset.
//! Every mutation creates an immutable [`MemoryVersion`] for audit and
//! point-in-time recovery.
//!
//! Limits: max 8 stores per session, individual memories capped at
//! 100KB (~25K tokens). Structure as many small focused files.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::MANAGED_AGENTS_BETA;

// =====================================================================
// Memory store types
// =====================================================================

/// A workspace-scoped collection of memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MemoryStore {
    /// Stable identifier (`memstore_...`).
    pub id: String,
    /// Wire type tag (`"memory_store"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Human-readable name.
    pub name: String,
    /// Description shown to the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Last-modified timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Set when the store has been archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Request body for `POST /v1/memory_stores`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateMemoryStoreRequest {
    /// Human-readable name. Required.
    pub name: String,
    /// Description shown to the agent describing what's in the store.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl CreateMemoryStoreRequest {
    /// Build a request with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }

    /// Attach a description.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Request body for `POST /v1/memory_stores/{id}` (update).
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateMemoryStoreRequest {
    /// New name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Optional knobs for [`MemoryStores::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListMemoryStoresParams {
    /// Pagination cursor.
    pub after: Option<String>,
    /// Pagination cursor.
    pub before: Option<String>,
    /// Page size.
    pub limit: Option<u32>,
    /// Whether to include archived stores.
    pub include_archived: Option<bool>,
}

impl ListMemoryStoresParams {
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

// =====================================================================
// Memory types
// =====================================================================

/// One memory inside a store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Memory {
    /// Stable identifier (`mem_...`).
    pub id: String,
    /// Wire `type` tag: `"file"` or `"directory"` for directory-style
    /// listings; preserved as-is.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Path within the store (e.g. `/preferences/formatting.md`).
    pub path: String,
    /// Memory content. Absent on list responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// SHA-256 of the current content. Used for optimistic-concurrency
    /// preconditions on update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    /// Size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Last-modified timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Request body for `POST /v1/memory_stores/{id}/memories`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateMemoryRequest {
    /// Path within the store.
    pub path: String,
    /// Initial content.
    pub content: String,
}

impl CreateMemoryRequest {
    /// Build with a path and content.
    #[must_use]
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
}

/// Optimistic-concurrency precondition for [`UpdateMemoryRequest`].
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryPrecondition {
    /// Apply only if the stored content hash matches.
    ContentSha256 {
        /// Expected hash.
        content_sha256: String,
    },
}

/// Request body for `POST /v1/memory_stores/{id}/memories/{mem}` (update).
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateMemoryRequest {
    /// New content. Pass `None` to keep the current content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// New path (rename).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Optimistic-concurrency precondition. The update is rejected if
    /// the stored hash no longer matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precondition: Option<MemoryPrecondition>,
}

/// Optional knobs for [`Memories::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListMemoriesParams {
    /// Browse by path prefix.
    pub path_prefix: Option<String>,
    /// Sort key (`"path"`, etc.).
    pub order_by: Option<String>,
    /// Maximum recursion depth when browsing.
    pub depth: Option<u32>,
    /// Pagination cursor.
    pub after: Option<String>,
    /// Page size.
    pub limit: Option<u32>,
}

impl ListMemoriesParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(p) = &self.path_prefix {
            q.push(("path_prefix", p.clone()));
        }
        if let Some(o) = &self.order_by {
            q.push(("order_by", o.clone()));
        }
        if let Some(d) = self.depth {
            q.push(("depth", d.to_string()));
        }
        if let Some(a) = &self.after {
            q.push(("after", a.clone()));
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        q
    }
}

// =====================================================================
// Memory version types
// =====================================================================

/// An immutable historical version of a memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MemoryVersion {
    /// Stable identifier (`memver_...`).
    pub id: String,
    /// ID of the memory this version belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    /// What kind of mutation produced this version: typically `create`,
    /// `update`, `delete`, or `redact`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Path at the time of this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Content at the time of this version. Absent on list responses;
    /// the retrieve endpoint includes it. `None` for redacted versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// `true` once the version has been redacted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted: Option<bool>,
    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Optional knobs for [`MemoryVersions::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListMemoryVersionsParams {
    /// Filter to a single memory's history.
    pub memory_id: Option<String>,
    /// Pagination cursor.
    pub after: Option<String>,
    /// Page size.
    pub limit: Option<u32>,
}

impl ListMemoryVersionsParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(m) = &self.memory_id {
            q.push(("memory_id", m.clone()));
        }
        if let Some(a) = &self.after {
            q.push(("after", a.clone()));
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        q
    }
}

// =====================================================================
// Namespace handles
// =====================================================================

/// Namespace handle for the memory-stores API.
pub struct MemoryStores<'a> {
    client: &'a Client,
}

impl<'a> MemoryStores<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/memory_stores`.
    pub async fn create(&self, request: CreateMemoryStoreRequest) -> Result<MemoryStore> {
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/memory_stores")
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/memory_stores/{id}`.
    pub async fn retrieve(&self, store_id: &str) -> Result<MemoryStore> {
        let path = format!("/v1/memory_stores/{store_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/memory_stores/{id}` (update name / description).
    pub async fn update(
        &self,
        store_id: &str,
        request: UpdateMemoryStoreRequest,
    ) -> Result<MemoryStore> {
        let path = format!("/v1/memory_stores/{store_id}");
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/memory_stores`.
    pub async fn list(&self, params: ListMemoryStoresParams) -> Result<Paginated<MemoryStore>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/memory_stores");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/memory_stores/{id}/archive`. One-way; there is no
    /// unarchive.
    pub async fn archive(&self, store_id: &str) -> Result<MemoryStore> {
        let path = format!("/v1/memory_stores/{store_id}/archive");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `DELETE /v1/memory_stores/{id}`. Permanently removes the store
    /// and all of its memories and versions. Use archive if you need
    /// an audit trail.
    pub async fn delete(&self, store_id: &str) -> Result<()> {
        let path = format!("/v1/memory_stores/{store_id}");
        let _: serde_json::Value = self
            .client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await?;
        Ok(())
    }

    /// Sub-namespace for memory operations on a single store.
    #[must_use]
    pub fn memories(&self, store_id: impl Into<String>) -> Memories<'_> {
        Memories {
            client: self.client,
            store_id: store_id.into(),
        }
    }

    /// Sub-namespace for version-history operations on a single store.
    #[must_use]
    pub fn memory_versions(&self, store_id: impl Into<String>) -> MemoryVersions<'_> {
        MemoryVersions {
            client: self.client,
            store_id: store_id.into(),
        }
    }
}

/// Namespace handle for memory operations on a single store.
pub struct Memories<'a> {
    client: &'a Client,
    store_id: String,
}

impl Memories<'_> {
    /// `POST /v1/memory_stores/{store_id}/memories`.
    pub async fn create(&self, request: CreateMemoryRequest) -> Result<Memory> {
        let path = format!("/v1/memory_stores/{}/memories", self.store_id);
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/memory_stores/{store_id}/memories/{memory_id}`.
    pub async fn retrieve(&self, memory_id: &str) -> Result<Memory> {
        let path = format!("/v1/memory_stores/{}/memories/{memory_id}", self.store_id);
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/memory_stores/{store_id}/memories/{memory_id}` (update).
    /// Pass an [`UpdateMemoryRequest::precondition`] for safe concurrent
    /// edits.
    pub async fn update(&self, memory_id: &str, request: UpdateMemoryRequest) -> Result<Memory> {
        let path = format!("/v1/memory_stores/{}/memories/{memory_id}", self.store_id);
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/memory_stores/{store_id}/memories`.
    pub async fn list(&self, params: ListMemoriesParams) -> Result<Paginated<Memory>> {
        let path = format!("/v1/memory_stores/{}/memories", self.store_id);
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
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `DELETE /v1/memory_stores/{store_id}/memories/{memory_id}`.
    pub async fn delete(&self, memory_id: &str) -> Result<()> {
        let path = format!("/v1/memory_stores/{}/memories/{memory_id}", self.store_id);
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

/// Namespace handle for memory-version (history) operations on a store.
pub struct MemoryVersions<'a> {
    client: &'a Client,
    store_id: String,
}

impl MemoryVersions<'_> {
    /// `GET /v1/memory_stores/{store_id}/memory_versions`.
    pub async fn list(&self, params: ListMemoryVersionsParams) -> Result<Paginated<MemoryVersion>> {
        let path = format!("/v1/memory_stores/{}/memory_versions", self.store_id);
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
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/memory_stores/{store_id}/memory_versions/{version_id}`.
    pub async fn retrieve(&self, version_id: &str) -> Result<MemoryVersion> {
        let path = format!(
            "/v1/memory_stores/{}/memory_versions/{version_id}",
            self.store_id
        );
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/memory_stores/{store_id}/memory_versions/{version_id}/redact`.
    /// Scrubs content while preserving the audit trail. Cannot redact
    /// the head version of a live memory; write a new version or
    /// delete the memory first.
    pub async fn redact(&self, version_id: &str) -> Result<MemoryVersion> {
        let path = format!(
            "/v1/memory_stores/{}/memory_versions/{version_id}/redact",
            self.store_id
        );
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(&serde_json::json!({}))
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn create_memory_store_round_trips() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/memory_stores"))
            .and(body_partial_json(json!({
                "name": "User Preferences",
                "description": "Per-user preferences."
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "memstore_01",
                "type": "memory_store",
                "name": "User Preferences",
                "description": "Per-user preferences."
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMemoryStoreRequest::new("User Preferences")
            .with_description("Per-user preferences.");
        let s = client
            .managed_agents()
            .memory_stores()
            .create(req)
            .await
            .unwrap();
        assert_eq!(s.id, "memstore_01");
    }

    #[tokio::test]
    async fn create_memory_under_store() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/memory_stores/memstore_01/memories"))
            .and(body_partial_json(json!({
                "path": "/preferences/formatting.md",
                "content": "Always use tabs."
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "mem_01",
                "type": "file",
                "path": "/preferences/formatting.md",
                "content": "Always use tabs.",
                "content_sha256": "abc123"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMemoryRequest::new("/preferences/formatting.md", "Always use tabs.");
        let m = client
            .managed_agents()
            .memory_stores()
            .memories("memstore_01")
            .create(req)
            .await
            .unwrap();
        assert_eq!(m.id, "mem_01");
        assert_eq!(m.content_sha256.as_deref(), Some("abc123"));
    }

    #[tokio::test]
    async fn update_memory_with_content_sha256_precondition() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/memory_stores/memstore_01/memories/mem_01"))
            .and(body_partial_json(json!({
                "content": "CORRECTED",
                "precondition": {"type": "content_sha256", "content_sha256": "abc123"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "mem_01",
                "path": "/preferences/formatting.md",
                "content": "CORRECTED",
                "content_sha256": "def456"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = UpdateMemoryRequest {
            content: Some("CORRECTED".into()),
            path: None,
            precondition: Some(MemoryPrecondition::ContentSha256 {
                content_sha256: "abc123".into(),
            }),
        };
        let m = client
            .managed_agents()
            .memory_stores()
            .memories("memstore_01")
            .update("mem_01", req)
            .await
            .unwrap();
        assert_eq!(m.content_sha256.as_deref(), Some("def456"));
    }

    #[tokio::test]
    async fn list_memories_passes_path_prefix_query() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/memory_stores/memstore_01/memories"))
            .and(wiremock::matchers::query_param(
                "path_prefix",
                "/preferences/",
            ))
            .and(wiremock::matchers::query_param("depth", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"id": "mem_01", "type": "file", "path": "/preferences/formatting.md"}
                ],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .memory_stores()
            .memories("memstore_01")
            .list(ListMemoriesParams {
                path_prefix: Some("/preferences/".into()),
                depth: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn retrieve_memory_store_returns_typed_record() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/memory_stores/memstore_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "memstore_01",
                "type": "memory_store",
                "name": "Prefs"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let s = client
            .managed_agents()
            .memory_stores()
            .retrieve("memstore_01")
            .await
            .unwrap();
        assert_eq!(s.id, "memstore_01");
    }

    #[tokio::test]
    async fn update_memory_store_patches_name_and_description() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/memory_stores/memstore_01"))
            .and(wiremock::matchers::body_partial_json(json!({
                "name": "Renamed",
                "description": "New desc."
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "memstore_01",
                "name": "Renamed",
                "description": "New desc."
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let s = client
            .managed_agents()
            .memory_stores()
            .update(
                "memstore_01",
                UpdateMemoryStoreRequest {
                    name: Some("Renamed".into()),
                    description: Some("New desc.".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(s.name, "Renamed");
    }

    #[tokio::test]
    async fn list_memory_stores_passes_include_archived_query() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/memory_stores"))
            .and(wiremock::matchers::query_param("include_archived", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "memstore_01", "name": "Prefs", "archived_at": "2026-04-30T12:00:00Z"}],
                "has_more": false
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .memory_stores()
            .list(ListMemoryStoresParams {
                include_archived: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
        assert!(page.data[0].archived_at.is_some());
    }

    #[tokio::test]
    async fn archive_memory_store_posts_to_archive_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/memory_stores/memstore_01/archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "memstore_01",
                "name": "Prefs",
                "archived_at": "2026-04-30T12:00:00Z"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let s = client
            .managed_agents()
            .memory_stores()
            .archive("memstore_01")
            .await
            .unwrap();
        assert!(s.archived_at.is_some());
    }

    #[tokio::test]
    async fn delete_memory_store_returns_unit() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/memory_stores/memstore_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .managed_agents()
            .memory_stores()
            .delete("memstore_01")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retrieve_memory_returns_full_content() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/memory_stores/memstore_01/memories/mem_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "mem_01",
                "type": "file",
                "path": "/notes.md",
                "content": "Hello.",
                "content_sha256": "abc"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let m = client
            .managed_agents()
            .memory_stores()
            .memories("memstore_01")
            .retrieve("mem_01")
            .await
            .unwrap();
        assert_eq!(m.content.as_deref(), Some("Hello."));
    }

    #[tokio::test]
    async fn delete_memory_returns_unit() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/memory_stores/memstore_01/memories/mem_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .managed_agents()
            .memory_stores()
            .memories("memstore_01")
            .delete("mem_01")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retrieve_memory_version_includes_content() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/v1/memory_stores/memstore_01/memory_versions/memver_01",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "memver_01",
                "memory_id": "mem_01",
                "operation": "create",
                "path": "/notes.md",
                "content": "Original."
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let v = client
            .managed_agents()
            .memory_stores()
            .memory_versions("memstore_01")
            .retrieve("memver_01")
            .await
            .unwrap();
        assert_eq!(v.content.as_deref(), Some("Original."));
        assert_eq!(v.operation.as_deref(), Some("create"));
    }

    #[tokio::test]
    async fn redact_memory_version_posts_to_redact_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/v1/memory_stores/memstore_01/memory_versions/memver_01/redact",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "memver_01",
                "operation": "redact",
                "redacted": true
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let v = client
            .managed_agents()
            .memory_stores()
            .memory_versions("memstore_01")
            .redact("memver_01")
            .await
            .unwrap();
        assert_eq!(v.redacted, Some(true));
    }

    #[tokio::test]
    async fn list_memory_versions_filters_by_memory_id() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/memory_stores/memstore_01/memory_versions"))
            .and(wiremock::matchers::query_param("memory_id", "mem_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"id": "memver_02", "operation": "update"},
                    {"id": "memver_01", "operation": "create"}
                ],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .memory_stores()
            .memory_versions("memstore_01")
            .list(ListMemoryVersionsParams {
                memory_id: Some("mem_01".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 2);
    }
}
