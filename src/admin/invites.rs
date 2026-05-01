//! Invites: create / retrieve / list / delete.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::{InviteStatus, ListParams, OrganizationRole, WriteOrganizationRole};

/// A pending or completed invite.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Invite {
    /// Stable invite ID.
    pub id: String,
    /// Wire type tag (`"invite"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Email of the invited user.
    pub email: String,
    /// Role assigned on accept.
    pub role: OrganizationRole,
    /// Lifecycle status.
    pub status: InviteStatus,
    /// RFC3339 timestamp when the invite was created.
    pub invited_at: String,
    /// RFC3339 timestamp when the invite expires.
    pub expires_at: String,
}

/// Response shape for `DELETE /v1/organizations/invites/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InviteDeleted {
    /// ID of the deleted invite.
    pub id: String,
    /// Wire type tag (`"invite_deleted"`).
    #[serde(rename = "type")]
    pub ty: String,
}

/// Request body for `POST /v1/organizations/invites`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateInviteRequest {
    /// Email to invite.
    pub email: String,
    /// Role to assign on accept. `admin` is not a valid value.
    pub role: WriteOrganizationRole,
}

impl CreateInviteRequest {
    /// Build a request.
    #[must_use]
    pub fn new(email: impl Into<String>, role: WriteOrganizationRole) -> Self {
        Self {
            email: email.into(),
            role,
        }
    }
}

/// Namespace handle for invite endpoints.
pub struct Invites<'a> {
    client: &'a Client,
}

impl<'a> Invites<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/organizations/invites`.
    pub async fn create(&self, request: CreateInviteRequest) -> Result<Invite> {
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/organizations/invites")
                        .json(body)
                },
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/invites/{id}`.
    pub async fn retrieve(&self, invite_id: &str) -> Result<Invite> {
        let path = format!("/v1/organizations/invites/{invite_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/invites`.
    pub async fn list(&self, params: ListParams) -> Result<Paginated<Invite>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/organizations/invites");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[],
            )
            .await
    }

    /// `DELETE /v1/organizations/invites/{id}`.
    pub async fn delete(&self, invite_id: &str) -> Result<InviteDeleted> {
        let path = format!("/v1/organizations/invites/{invite_id}");
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

    fn fake_invite() -> serde_json::Value {
        json!({
            "id": "invite_01",
            "type": "invite",
            "email": "u@example.com",
            "role": "user",
            "status": "pending",
            "invited_at": "2026-05-01T00:00:00Z",
            "expires_at": "2026-05-08T00:00:00Z"
        })
    }

    #[test]
    fn organization_role_unknown_falls_through_to_other() {
        let raw = json!("future_role");
        let r: OrganizationRole = serde_json::from_value(raw).unwrap();
        assert_eq!(r, OrganizationRole::Other("future_role".into()));
    }

    #[test]
    fn organization_role_round_trips_known_variants() {
        for v in ["user", "developer", "billing", "admin", "claude_code_user"] {
            let r: OrganizationRole = serde_json::from_value(json!(v)).unwrap();
            let s = serde_json::to_value(&r).unwrap();
            assert_eq!(s, json!(v));
        }
    }

    #[tokio::test]
    async fn create_invite_posts_email_and_role() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/invites"))
            .and(body_partial_json(json!({
                "email": "u@example.com",
                "role": "developer"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_invite()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let inv = client
            .admin()
            .invites()
            .create(CreateInviteRequest::new(
                "u@example.com",
                WriteOrganizationRole::Developer,
            ))
            .await
            .unwrap();
        assert_eq!(inv.id, "invite_01");
    }

    #[tokio::test]
    async fn retrieve_invite_returns_typed_record() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/invites/invite_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_invite()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let inv = client
            .admin()
            .invites()
            .retrieve("invite_01")
            .await
            .unwrap();
        assert_eq!(inv.email, "u@example.com");
    }

    #[tokio::test]
    async fn list_invites_passes_pagination_query() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/invites"))
            .and(wiremock::matchers::query_param("limit", "50"))
            .and(wiremock::matchers::query_param("after_id", "invite_x"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [fake_invite()],
                "has_more": false,
                "first_id": "invite_01",
                "last_id": "invite_01"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .admin()
            .invites()
            .list(ListParams {
                after_id: Some("invite_x".into()),
                limit: Some(50),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn delete_invite_returns_deleted_response() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/organizations/invites/invite_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "invite_01",
                "type": "invite_deleted"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let r = client.admin().invites().delete("invite_01").await.unwrap();
        assert_eq!(r.ty, "invite_deleted");
    }
}
