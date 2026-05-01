//! Workspace members: create / retrieve / list / update / delete.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::{ListParams, WorkspaceRole, WriteWorkspaceRole};

/// One workspace-member assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WorkspaceMember {
    /// Wire type tag (`"workspace_member"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// User ID.
    pub user_id: String,
    /// Workspace ID.
    pub workspace_id: String,
    /// Member's role in this workspace.
    pub workspace_role: WorkspaceRole,
}

/// Response shape for `DELETE
/// /v1/organizations/workspaces/{ws}/members/{user}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WorkspaceMemberDeleted {
    /// User ID.
    pub user_id: String,
    /// Workspace ID.
    pub workspace_id: String,
    /// Wire type tag (`"workspace_member_deleted"`).
    #[serde(rename = "type")]
    pub ty: String,
}

/// Request body for `POST /v1/organizations/workspaces/{ws}/members`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateWorkspaceMemberRequest {
    /// User to add.
    pub user_id: String,
    /// Role to assign. `workspace_billing` is not a valid value.
    pub workspace_role: WriteWorkspaceRole,
}

impl CreateWorkspaceMemberRequest {
    /// Build a request.
    #[must_use]
    pub fn new(user_id: impl Into<String>, role: WriteWorkspaceRole) -> Self {
        Self {
            user_id: user_id.into(),
            workspace_role: role,
        }
    }
}

/// Request body for `POST /v1/organizations/workspaces/{ws}/members/{user}`
/// (update role).
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct UpdateWorkspaceMemberRequest {
    /// New role. Per the API the body type allows `workspace_billing`
    /// here; use [`WorkspaceRole`] directly so callers can pass it.
    pub workspace_role: WorkspaceRole,
}

impl UpdateWorkspaceMemberRequest {
    /// Build with a new role.
    #[must_use]
    pub fn new(role: WorkspaceRole) -> Self {
        Self {
            workspace_role: role,
        }
    }
}

/// Namespace handle for workspace-member endpoints. Scoped to a single
/// workspace at construction time.
pub struct WorkspaceMembers<'a> {
    client: &'a Client,
    workspace_id: String,
}

impl<'a> WorkspaceMembers<'a> {
    pub(crate) fn new(client: &'a Client, workspace_id: String) -> Self {
        Self {
            client,
            workspace_id,
        }
    }

    /// `POST /v1/organizations/workspaces/{ws}/members`.
    pub async fn create(&self, request: CreateWorkspaceMemberRequest) -> Result<WorkspaceMember> {
        let path = format!("/v1/organizations/workspaces/{}/members", self.workspace_id);
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

    /// `GET /v1/organizations/workspaces/{ws}/members/{user}`.
    pub async fn retrieve(&self, user_id: &str) -> Result<WorkspaceMember> {
        let path = format!(
            "/v1/organizations/workspaces/{}/members/{user_id}",
            self.workspace_id
        );
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/workspaces/{ws}/members`.
    pub async fn list(&self, params: ListParams) -> Result<Paginated<WorkspaceMember>> {
        let path = format!("/v1/organizations/workspaces/{}/members", self.workspace_id);
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

    /// `POST /v1/organizations/workspaces/{ws}/members/{user}` (update).
    pub async fn update(
        &self,
        user_id: &str,
        request: UpdateWorkspaceMemberRequest,
    ) -> Result<WorkspaceMember> {
        let path = format!(
            "/v1/organizations/workspaces/{}/members/{user_id}",
            self.workspace_id
        );
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

    /// `DELETE /v1/organizations/workspaces/{ws}/members/{user}`.
    pub async fn delete(&self, user_id: &str) -> Result<WorkspaceMemberDeleted> {
        let path = format!(
            "/v1/organizations/workspaces/{}/members/{user_id}",
            self.workspace_id
        );
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                &[],
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn fake_member() -> serde_json::Value {
        json!({
            "type": "workspace_member",
            "user_id": "user_01",
            "workspace_id": "ws_01",
            "workspace_role": "workspace_user"
        })
    }

    #[tokio::test]
    async fn create_workspace_member_posts_user_id_and_role() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/workspaces/ws_01/members"))
            .and(body_partial_json(json!({
                "user_id": "user_01",
                "workspace_role": "workspace_admin"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_member()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .admin()
            .workspace_members("ws_01")
            .create(CreateWorkspaceMemberRequest::new(
                "user_01",
                WriteWorkspaceRole::WorkspaceAdmin,
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retrieve_workspace_member_returns_role() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/workspaces/ws_01/members/user_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_member()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let m = client
            .admin()
            .workspace_members("ws_01")
            .retrieve("user_01")
            .await
            .unwrap();
        assert!(matches!(m.workspace_role, WorkspaceRole::User));
    }

    #[tokio::test]
    async fn list_workspace_members_paginates() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/workspaces/ws_01/members"))
            .and(wiremock::matchers::query_param("limit", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [fake_member()],
                "has_more": false,
                "first_id": "user_01",
                "last_id": "user_01"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .admin()
            .workspace_members("ws_01")
            .list(ListParams {
                limit: Some(5),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn update_workspace_member_changes_role() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/workspaces/ws_01/members/user_01"))
            .and(body_partial_json(json!({
                "workspace_role": "workspace_developer"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_member()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .admin()
            .workspace_members("ws_01")
            .update(
                "user_01",
                UpdateWorkspaceMemberRequest::new(WorkspaceRole::Developer),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_workspace_member_returns_deleted_marker() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/organizations/workspaces/ws_01/members/user_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "type": "workspace_member_deleted",
                "user_id": "user_01",
                "workspace_id": "ws_01"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let r = client
            .admin()
            .workspace_members("ws_01")
            .delete("user_01")
            .await
            .unwrap();
        assert_eq!(r.ty, "workspace_member_deleted");
    }
}
