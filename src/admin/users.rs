//! Users: retrieve / list / update / delete.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::{ListParams, OrganizationRole, WriteOrganizationRole};

/// An organization user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct User {
    /// Stable user ID.
    pub id: String,
    /// Wire type tag (`"user"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Email.
    pub email: String,
    /// Display name.
    pub name: String,
    /// Role.
    pub role: OrganizationRole,
    /// RFC3339 timestamp when the user joined.
    pub added_at: String,
}

/// Response shape for `DELETE /v1/organizations/users/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserDeleted {
    /// ID of the deleted user.
    pub id: String,
    /// Wire type tag (`"user_deleted"`).
    #[serde(rename = "type")]
    pub ty: String,
}

/// Request body for `POST /v1/organizations/users/{id}` (update).
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct UpdateUserRequest {
    /// New role. Cannot be `admin`.
    pub role: WriteOrganizationRole,
}

impl UpdateUserRequest {
    /// Build with a new role.
    #[must_use]
    pub fn new(role: WriteOrganizationRole) -> Self {
        Self { role }
    }
}

/// Filters for [`Users::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListUsersParams {
    /// Underlying pagination params.
    pub paging: ListParams,
    /// Filter to a specific email.
    pub email: Option<String>,
}

impl ListUsersParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = self.paging.to_query();
        if let Some(e) = &self.email {
            q.push(("email", e.clone()));
        }
        q
    }
}

/// Namespace handle for user endpoints.
pub struct Users<'a> {
    client: &'a Client,
}

impl<'a> Users<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /v1/organizations/users/{id}`.
    pub async fn retrieve(&self, user_id: &str) -> Result<User> {
        let path = format!("/v1/organizations/users/{user_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/users`.
    pub async fn list(&self, params: ListUsersParams) -> Result<Paginated<User>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/organizations/users");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[],
            )
            .await
    }

    /// `POST /v1/organizations/users/{id}` (update role).
    pub async fn update(&self, user_id: &str, request: UpdateUserRequest) -> Result<User> {
        let path = format!("/v1/organizations/users/{user_id}");
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

    /// `DELETE /v1/organizations/users/{id}`.
    pub async fn delete(&self, user_id: &str) -> Result<UserDeleted> {
        let path = format!("/v1/organizations/users/{user_id}");
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

    fn fake_user() -> serde_json::Value {
        json!({
            "id": "user_01",
            "type": "user",
            "email": "u@example.com",
            "name": "User",
            "role": "developer",
            "added_at": "2026-05-01T00:00:00Z"
        })
    }

    #[tokio::test]
    async fn retrieve_user_returns_typed_record() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/users/user_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_user()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let u = client.admin().users().retrieve("user_01").await.unwrap();
        assert_eq!(u.email, "u@example.com");
    }

    #[tokio::test]
    async fn list_users_filters_by_email() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/users"))
            .and(wiremock::matchers::query_param("email", "u@example.com"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [fake_user()],
                "has_more": false,
                "first_id": "user_01",
                "last_id": "user_01"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .admin()
            .users()
            .list(ListUsersParams {
                email: Some("u@example.com".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn update_user_role_sends_role_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/users/user_01"))
            .and(body_partial_json(json!({"role": "user"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_user()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .admin()
            .users()
            .update(
                "user_01",
                UpdateUserRequest::new(WriteOrganizationRole::User),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_user_returns_deleted_marker() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/organizations/users/user_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "user_01",
                "type": "user_deleted"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let r = client.admin().users().delete("user_01").await.unwrap();
        assert_eq!(r.ty, "user_deleted");
    }
}
