//! Workspaces: create / retrieve / list / update / archive.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::ListParams;

/// Permitted-inference-geos value: either `"unrestricted"` (allow
/// every geo) or an explicit list.
///
/// Forward-compatible: the wire form is either the literal string
/// `"unrestricted"` or an array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum AllowedInferenceGeos {
    /// Unrestricted (string sentinel).
    Unrestricted(UnrestrictedSentinel),
    /// Explicit allow-list of geo codes.
    List(Vec<String>),
}

/// Type-tag witness for [`AllowedInferenceGeos::Unrestricted`]: the
/// literal string `"unrestricted"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UnrestrictedSentinel {
    /// Always serializes as `"unrestricted"`.
    #[serde(rename = "unrestricted")]
    Unrestricted,
}

impl AllowedInferenceGeos {
    /// Build the `"unrestricted"` form.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self::Unrestricted(UnrestrictedSentinel::Unrestricted)
    }

    /// Build an explicit allow-list.
    #[must_use]
    pub fn list<I, S>(geos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::List(geos.into_iter().map(Into::into).collect())
    }
}

/// Data residency configuration on a [`Workspace`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DataResidency {
    /// Permitted inference geos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_inference_geos: Option<AllowedInferenceGeos>,
    /// Default geo applied when a request omits the parameter.
    /// Defaults server-side to `"global"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_inference_geo: Option<String>,
    /// Geographic region for workspace data storage. **Immutable**
    /// after creation. Defaults to `"us"` server-side. Only present
    /// on response payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_geo: Option<String>,
}

/// A workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Workspace {
    /// Stable workspace ID.
    pub id: String,
    /// Wire type tag (`"workspace"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Display name.
    pub name: String,
    /// Hex color code shown in the Console.
    pub display_color: String,
    /// Data residency configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_residency: Option<DataResidency>,
    /// Creation timestamp.
    pub created_at: String,
    /// Set when archived; `None` for live workspaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Request body for `POST /v1/organizations/workspaces`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateWorkspaceRequest {
    /// Workspace name.
    pub name: String,
    /// Optional data residency. `workspace_geo` is immutable after
    /// creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_residency: Option<DataResidency>,
}

impl CreateWorkspaceRequest {
    /// Build with the given name; default residency.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data_residency: None,
        }
    }

    /// Attach a residency configuration.
    #[must_use]
    pub fn with_data_residency(mut self, residency: DataResidency) -> Self {
        self.data_residency = Some(residency);
        self
    }
}

/// Request body for `POST /v1/organizations/workspaces/{id}` (update).
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct UpdateWorkspaceRequest {
    /// New name.
    pub name: String,
    /// Optional residency patch (cannot change `workspace_geo`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_residency: Option<DataResidency>,
}

impl UpdateWorkspaceRequest {
    /// Build with the new name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data_residency: None,
        }
    }

    /// Attach a residency patch.
    #[must_use]
    pub fn with_data_residency(mut self, residency: DataResidency) -> Self {
        self.data_residency = Some(residency);
        self
    }
}

/// Filters for [`Workspaces::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListWorkspacesParams {
    /// Underlying pagination params.
    pub paging: ListParams,
    /// Whether to include archived workspaces.
    pub include_archived: Option<bool>,
}

impl ListWorkspacesParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = self.paging.to_query();
        if let Some(b) = self.include_archived {
            q.push(("include_archived", b.to_string()));
        }
        q
    }
}

/// Namespace handle for workspace endpoints.
pub struct Workspaces<'a> {
    client: &'a Client,
}

impl<'a> Workspaces<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/organizations/workspaces`.
    pub async fn create(&self, request: CreateWorkspaceRequest) -> Result<Workspace> {
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/organizations/workspaces")
                        .json(body)
                },
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/workspaces/{id}`.
    pub async fn retrieve(&self, workspace_id: &str) -> Result<Workspace> {
        let path = format!("/v1/organizations/workspaces/{workspace_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/workspaces`.
    pub async fn list(&self, params: ListWorkspacesParams) -> Result<Paginated<Workspace>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/organizations/workspaces");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[],
            )
            .await
    }

    /// `POST /v1/organizations/workspaces/{id}` (update).
    pub async fn update(
        &self,
        workspace_id: &str,
        request: UpdateWorkspaceRequest,
    ) -> Result<Workspace> {
        let path = format!("/v1/organizations/workspaces/{workspace_id}");
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(body)
                },
                &[],
            )
            .await
    }

    /// `POST /v1/organizations/workspaces/{id}/archive`.
    pub async fn archive(&self, workspace_id: &str) -> Result<Workspace> {
        let path = format!("/v1/organizations/workspaces/{workspace_id}/archive");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[],
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
            .api_key("sk-ant-admin-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn fake_workspace() -> serde_json::Value {
        json!({
            "id": "ws_01",
            "type": "workspace",
            "name": "Default",
            "display_color": "#0a84ff",
            "created_at": "2026-05-01T00:00:00Z"
        })
    }

    #[test]
    fn allowed_inference_geos_serializes_unrestricted_as_string() {
        let v = serde_json::to_value(AllowedInferenceGeos::unrestricted()).unwrap();
        assert_eq!(v, json!("unrestricted"));
    }

    #[test]
    fn allowed_inference_geos_serializes_list_form() {
        let v = serde_json::to_value(AllowedInferenceGeos::list(["us", "eu"])).unwrap();
        assert_eq!(v, json!(["us", "eu"]));
    }

    #[test]
    fn allowed_inference_geos_round_trips_both_forms() {
        let s: AllowedInferenceGeos = serde_json::from_value(json!("unrestricted")).unwrap();
        assert_eq!(s, AllowedInferenceGeos::unrestricted());
        let l: AllowedInferenceGeos = serde_json::from_value(json!(["us"])).unwrap();
        assert_eq!(l, AllowedInferenceGeos::list(["us"]));
    }

    #[tokio::test]
    async fn create_workspace_minimal_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/workspaces"))
            .and(body_partial_json(json!({"name": "Default"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_workspace()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let w = client
            .admin()
            .workspaces()
            .create(CreateWorkspaceRequest::new("Default"))
            .await
            .unwrap();
        assert_eq!(w.id, "ws_01");
    }

    #[tokio::test]
    async fn list_workspaces_passes_include_archived() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/workspaces"))
            .and(wiremock::matchers::query_param("include_archived", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [fake_workspace()],
                "has_more": false,
                "first_id": "ws_01",
                "last_id": "ws_01"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .admin()
            .workspaces()
            .list(ListWorkspacesParams {
                include_archived: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn update_workspace_round_trips() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/workspaces/ws_01"))
            .and(body_partial_json(json!({"name": "Renamed"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_workspace()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .admin()
            .workspaces()
            .update("ws_01", UpdateWorkspaceRequest::new("Renamed"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn archive_workspace_posts_to_archive_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/workspaces/ws_01/archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json({
                let mut w = fake_workspace();
                w["archived_at"] = json!("2026-05-01T12:00:00Z");
                w
            }))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let w = client.admin().workspaces().archive("ws_01").await.unwrap();
        assert!(w.archived_at.is_some());
    }

    #[tokio::test]
    async fn retrieve_workspace_returns_typed_record() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/workspaces/ws_R1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_workspace()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let w = client.admin().workspaces().retrieve("ws_R1").await.unwrap();
        // fake_workspace() always returns id=ws_01 regardless of path.
        assert_eq!(w.id, "ws_01");
    }
}
