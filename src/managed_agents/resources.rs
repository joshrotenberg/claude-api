//! Session resources: file / `github_repository` / `memory_store` mounts.
//!
//! Resources are attached to a session at creation time (via
//! [`CreateSessionRequest::resources`](super::sessions::CreateSessionRequest::resources))
//! or added afterwards via the [`Resources`] sub-namespace. Each
//! resource has a server-assigned ID (`sesrsc_...`) used for update
//! and delete operations.
//!
//! Three known resource kinds are typed below; an unknown kind on the
//! wire deserializes into [`SessionResource::Other`] preserving the
//! raw JSON.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::MANAGED_AGENTS_BETA;

// =====================================================================
// Resource types
// =====================================================================

/// One resource mounted into a session container.
///
/// Forward-compatible: unknown wire `type` tags fall through to
/// [`Self::Other`] preserving the raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionResource {
    /// File mounted from the [Files API](crate::files).
    File(FileResource),
    /// GitHub repository cloned into the container.
    GitHubRepository(GitHubRepositoryResource),
    /// Memory store mounted under `/mnt/memory/`.
    MemoryStore(MemoryStoreResource),
    /// Unknown resource kind; raw JSON preserved.
    Other(serde_json::Value),
}

/// `type: "file"` resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileResource {
    /// Server-assigned resource ID, present on responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Files API ID of the uploaded file.
    pub file_id: String,
    /// Optional mount path inside the container. The server picks a
    /// path under the working directory when omitted; pass an explicit
    /// path for predictable references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_path: Option<String>,
}

impl FileResource {
    /// Build a file mount with an optional explicit mount path.
    #[must_use]
    pub fn new(file_id: impl Into<String>) -> Self {
        Self {
            id: None,
            file_id: file_id.into(),
            mount_path: None,
        }
    }

    /// Set an explicit mount path.
    #[must_use]
    pub fn mount_path(mut self, path: impl Into<String>) -> Self {
        self.mount_path = Some(path.into());
        self
    }
}

/// `type: "github_repository"` resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GitHubRepositoryResource {
    /// Server-assigned resource ID, present on responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// HTTPS URL of the repository.
    pub url: String,
    /// Mount path inside the container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_path: Option<String>,
    /// GitHub access token. **Write-only**: the server stores this
    /// internally and never echoes it on responses, so this field is
    /// always `None` on retrieved resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_token: Option<String>,
}

impl GitHubRepositoryResource {
    /// Build a repository mount.
    #[must_use]
    pub fn new(url: impl Into<String>, authorization_token: impl Into<String>) -> Self {
        Self {
            id: None,
            url: url.into(),
            mount_path: None,
            authorization_token: Some(authorization_token.into()),
        }
    }

    /// Set an explicit mount path.
    #[must_use]
    pub fn mount_path(mut self, path: impl Into<String>) -> Self {
        self.mount_path = Some(path.into());
        self
    }
}

/// Access mode for a [`MemoryStoreResource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryStoreAccess {
    /// Reference material the agent can read but not write.
    ReadOnly,
    /// Default. Writes produce new memory versions attributed to the
    /// session.
    ReadWrite,
}

/// `type: "memory_store"` resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MemoryStoreResource {
    /// Server-assigned resource ID, present on responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// ID of the memory store to mount.
    pub memory_store_id: String,
    /// Access mode. Defaults to `read_write` server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<MemoryStoreAccess>,
    /// Optional session-specific instructions for how the agent should
    /// use this store. Capped at 4,096 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl MemoryStoreResource {
    /// Build a memory-store mount with default access.
    #[must_use]
    pub fn new(memory_store_id: impl Into<String>) -> Self {
        Self {
            id: None,
            memory_store_id: memory_store_id.into(),
            access: None,
            instructions: None,
        }
    }

    /// Set explicit access.
    #[must_use]
    pub fn access(mut self, access: MemoryStoreAccess) -> Self {
        self.access = Some(access);
        self
    }

    /// Set session-specific instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }
}

const KNOWN_RESOURCE_TAGS: &[&str] = &["file", "github_repository", "memory_store"];

impl Serialize for SessionResource {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::File(r) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "file")?;
                if let Some(id) = &r.id {
                    map.serialize_entry("id", id)?;
                }
                map.serialize_entry("file_id", &r.file_id)?;
                if let Some(mp) = &r.mount_path {
                    map.serialize_entry("mount_path", mp)?;
                }
                map.end()
            }
            Self::GitHubRepository(r) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "github_repository")?;
                if let Some(id) = &r.id {
                    map.serialize_entry("id", id)?;
                }
                map.serialize_entry("url", &r.url)?;
                if let Some(mp) = &r.mount_path {
                    map.serialize_entry("mount_path", mp)?;
                }
                if let Some(t) = &r.authorization_token {
                    map.serialize_entry("authorization_token", t)?;
                }
                map.end()
            }
            Self::MemoryStore(r) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "memory_store")?;
                if let Some(id) = &r.id {
                    map.serialize_entry("id", id)?;
                }
                map.serialize_entry("memory_store_id", &r.memory_store_id)?;
                if let Some(a) = r.access {
                    map.serialize_entry("access", &a)?;
                }
                if let Some(i) = &r.instructions {
                    map.serialize_entry("instructions", i)?;
                }
                map.end()
            }
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for SessionResource {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let tag = raw.get("type").and_then(serde_json::Value::as_str);
        match tag {
            Some("file") if KNOWN_RESOURCE_TAGS.contains(&"file") => {
                let r = serde_json::from_value::<FileResource>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::File(r))
            }
            Some("github_repository") => {
                let r = serde_json::from_value::<GitHubRepositoryResource>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::GitHubRepository(r))
            }
            Some("memory_store") => {
                let r = serde_json::from_value::<MemoryStoreResource>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::MemoryStore(r))
            }
            _ => Ok(Self::Other(raw)),
        }
    }
}

impl SessionResource {
    /// Server-assigned resource ID, if any.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::File(r) => r.id.as_deref(),
            Self::GitHubRepository(r) => r.id.as_deref(),
            Self::MemoryStore(r) => r.id.as_deref(),
            Self::Other(v) => v.get("id").and_then(serde_json::Value::as_str),
        }
    }
}

// =====================================================================
// Update payloads
// =====================================================================

/// Patch for an existing session resource. Currently only the
/// `github_repository`'s `authorization_token` is mutable.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateResourceRequest {
    /// New GitHub access token. Leave `None` to make no change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_token: Option<String>,
}

impl UpdateResourceRequest {
    /// Build a request that rotates the GitHub authorization token.
    #[must_use]
    pub fn rotate_authorization_token(token: impl Into<String>) -> Self {
        Self {
            authorization_token: Some(token.into()),
        }
    }
}

// =====================================================================
// Namespace handle
// =====================================================================

/// Namespace handle for resource operations on a single session.
///
/// Obtained via
/// [`Sessions::resources`](super::sessions::Sessions::resources).
pub struct Resources<'a> {
    pub(crate) client: &'a Client,
    pub(crate) session_id: String,
}

impl Resources<'_> {
    /// `GET /v1/sessions/{session_id}/resources`.
    pub async fn list(&self) -> Result<Paginated<SessionResource>> {
        let path = format!("/v1/sessions/{}/resources", self.session_id);
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/sessions/{session_id}/resources`. Add a resource to a
    /// running session.
    pub async fn add(&self, resource: &SessionResource) -> Result<SessionResource> {
        let path = format!("/v1/sessions/{}/resources", self.session_id);
        let body = resource;
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

    /// `POST /v1/sessions/{session_id}/resources/{resource_id}`. Used
    /// to rotate a `github_repository` resource's `authorization_token`.
    pub async fn update(
        &self,
        resource_id: &str,
        request: UpdateResourceRequest,
    ) -> Result<SessionResource> {
        let path = format!("/v1/sessions/{}/resources/{resource_id}", self.session_id);
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

    /// `DELETE /v1/sessions/{session_id}/resources/{resource_id}`.
    pub async fn delete(&self, resource_id: &str) -> Result<()> {
        let path = format!("/v1/sessions/{}/resources/{resource_id}", self.session_id);
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
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[test]
    fn file_resource_round_trips_with_mount_path() {
        let r =
            SessionResource::File(FileResource::new("file_01").mount_path("/workspace/data.csv"));
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "file",
                "file_id": "file_01",
                "mount_path": "/workspace/data.csv"
            })
        );
        let parsed: SessionResource = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn github_resource_serializes_authorization_token_on_create() {
        let r = SessionResource::GitHubRepository(
            GitHubRepositoryResource::new("https://github.com/org/repo", "ghp_xxx")
                .mount_path("/workspace/repo"),
        );
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["authorization_token"], "ghp_xxx");
        assert_eq!(v["mount_path"], "/workspace/repo");
    }

    #[test]
    fn memory_store_resource_round_trips_with_access_and_instructions() {
        let r = SessionResource::MemoryStore(
            MemoryStoreResource::new("memstore_01")
                .access(MemoryStoreAccess::ReadOnly)
                .instructions("Reference only."),
        );
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "memory_store",
                "memory_store_id": "memstore_01",
                "access": "read_only",
                "instructions": "Reference only."
            })
        );
        let parsed: SessionResource = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn unknown_resource_type_falls_through_to_other() {
        let raw = json!({"type": "future_resource", "blob": [1, 2]});
        let parsed: SessionResource = serde_json::from_value(raw.clone()).unwrap();
        match parsed {
            SessionResource::Other(v) => assert_eq!(v, raw),
            SessionResource::File(_)
            | SessionResource::GitHubRepository(_)
            | SessionResource::MemoryStore(_) => panic!("expected Other"),
        }
    }

    #[tokio::test]
    async fn list_resources_returns_typed_session_resources() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_x/resources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"type": "file", "id": "sesrsc_a", "file_id": "file_01"},
                    {"type": "github_repository", "id": "sesrsc_b", "url": "https://github.com/o/r"}
                ],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .sessions()
            .resources("sesn_x")
            .list()
            .await
            .unwrap();
        assert_eq!(page.data.len(), 2);
        assert!(matches!(page.data[0], SessionResource::File(_)));
        assert!(matches!(page.data[1], SessionResource::GitHubRepository(_)));
    }

    #[tokio::test]
    async fn add_resource_posts_typed_payload() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions/sesn_x/resources"))
            .and(body_partial_json(json!({
                "type": "file",
                "file_id": "file_42"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "type": "file",
                "id": "sesrsc_42",
                "file_id": "file_42"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let added = client
            .managed_agents()
            .sessions()
            .resources("sesn_x")
            .add(&SessionResource::File(FileResource::new("file_42")))
            .await
            .unwrap();
        assert_eq!(added.id().unwrap(), "sesrsc_42");
    }

    #[tokio::test]
    async fn update_resource_rotates_authorization_token() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions/sesn_x/resources/sesrsc_b"))
            .and(body_partial_json(json!({
                "authorization_token": "ghp_new"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "type": "github_repository",
                "id": "sesrsc_b",
                "url": "https://github.com/o/r"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let _ = client
            .managed_agents()
            .sessions()
            .resources("sesn_x")
            .update(
                "sesrsc_b",
                UpdateResourceRequest::rotate_authorization_token("ghp_new"),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_resource_returns_unit_on_success() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/sessions/sesn_x/resources/sesrsc_b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        client
            .managed_agents()
            .sessions()
            .resources("sesn_x")
            .delete("sesrsc_b")
            .await
            .unwrap();
    }
}
