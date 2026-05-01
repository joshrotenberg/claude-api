//! API keys: retrieve / list / update.
//!
//! Note: the Admin API does **not** expose create / delete for API
//! keys -- those live in the Console UI. Update can change the name or
//! lifecycle status (`active` / `inactive` / `archived`).

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::ListParams;

/// Lifecycle status of an API key (response).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApiKeyStatus {
    /// In use.
    Active,
    /// Disabled but recoverable.
    Inactive,
    /// Archived (cannot be reactivated).
    Archived,
    /// Past its expiration timestamp.
    Expired,
}

/// Subset of [`ApiKeyStatus`] valid as a write value (no `expired`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WriteApiKeyStatus {
    /// Mark active.
    Active,
    /// Disable.
    Inactive,
    /// Archive permanently.
    Archived,
}

/// Actor (user or service account) that created an API key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ApiKeyCreator {
    /// Stable actor ID.
    pub id: String,
    /// Actor type (e.g. `"user"`, `"service_account"`). Free-form
    /// string; new types can appear without a wrapper enum needed.
    #[serde(rename = "type")]
    pub ty: String,
}

/// An API key record (the secret value itself is never returned).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ApiKey {
    /// Stable key ID.
    pub id: String,
    /// Wire type tag (`"api_key"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Display name.
    pub name: String,
    /// Partially redacted hint (e.g. `sk-ant-…abcd`).
    pub partial_key_hint: String,
    /// Lifecycle status.
    pub status: ApiKeyStatus,
    /// Workspace this key belongs to. `None` for the default
    /// workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Who created the key.
    pub created_by: ApiKeyCreator,
    /// Creation timestamp.
    pub created_at: String,
    /// Expiration timestamp. `None` if the key never expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Request body for `POST /v1/organizations/api_keys/{id}` (update).
/// Both fields are optional; pass `None` to leave a field unchanged.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateApiKeyRequest {
    /// New name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<WriteApiKeyStatus>,
}

impl UpdateApiKeyRequest {
    /// Empty patch; chain setters to populate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Update the status.
    #[must_use]
    pub fn status(mut self, status: WriteApiKeyStatus) -> Self {
        self.status = Some(status);
        self
    }
}

/// Filters for [`ApiKeys::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListApiKeysParams {
    /// Underlying pagination params.
    pub paging: ListParams,
    /// Filter by creator.
    pub created_by_user_id: Option<String>,
    /// Filter by status.
    pub status: Option<ApiKeyStatus>,
    /// Filter by workspace.
    pub workspace_id: Option<String>,
}

impl ListApiKeysParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = self.paging.to_query();
        if let Some(u) = &self.created_by_user_id {
            q.push(("created_by_user_id", u.clone()));
        }
        if let Some(s) = self.status {
            q.push((
                "status",
                match s {
                    ApiKeyStatus::Active => "active".into(),
                    ApiKeyStatus::Inactive => "inactive".into(),
                    ApiKeyStatus::Archived => "archived".into(),
                    ApiKeyStatus::Expired => "expired".into(),
                },
            ));
        }
        if let Some(w) = &self.workspace_id {
            q.push(("workspace_id", w.clone()));
        }
        q
    }
}

/// Namespace handle for API-key endpoints.
pub struct ApiKeys<'a> {
    client: &'a Client,
}

impl<'a> ApiKeys<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /v1/organizations/api_keys/{id}`.
    pub async fn retrieve(&self, key_id: &str) -> Result<ApiKey> {
        let path = format!("/v1/organizations/api_keys/{key_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[],
            )
            .await
    }

    /// `GET /v1/organizations/api_keys`.
    pub async fn list(&self, params: ListApiKeysParams) -> Result<Paginated<ApiKey>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/organizations/api_keys");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[],
            )
            .await
    }

    /// `POST /v1/organizations/api_keys/{id}` (update name/status).
    pub async fn update(&self, key_id: &str, request: UpdateApiKeyRequest) -> Result<ApiKey> {
        let path = format!("/v1/organizations/api_keys/{key_id}");
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

    fn fake_api_key() -> serde_json::Value {
        json!({
            "id": "apikey_01",
            "type": "api_key",
            "name": "ci",
            "partial_key_hint": "sk-ant-...abcd",
            "status": "active",
            "workspace_id": null,
            "created_by": {"id": "user_01", "type": "user"},
            "created_at": "2026-05-01T00:00:00Z",
            "expires_at": null
        })
    }

    #[tokio::test]
    async fn retrieve_api_key_returns_typed_record() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/api_keys/apikey_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_api_key()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let k = client
            .admin()
            .api_keys()
            .retrieve("apikey_01")
            .await
            .unwrap();
        assert_eq!(k.id, "apikey_01");
        assert_eq!(k.status, ApiKeyStatus::Active);
        assert_eq!(k.created_by.ty, "user");
    }

    #[tokio::test]
    async fn list_api_keys_filters_by_status_and_workspace() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/api_keys"))
            .and(wiremock::matchers::query_param("status", "active"))
            .and(wiremock::matchers::query_param("workspace_id", "ws_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [fake_api_key()],
                "has_more": false,
                "first_id": "apikey_01",
                "last_id": "apikey_01"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .admin()
            .api_keys()
            .list(ListApiKeysParams {
                status: Some(ApiKeyStatus::Active),
                workspace_id: Some("ws_01".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn update_api_key_can_change_name_and_status() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/organizations/api_keys/apikey_01"))
            .and(body_partial_json(json!({
                "name": "renamed",
                "status": "archived"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_api_key()))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .admin()
            .api_keys()
            .update(
                "apikey_01",
                UpdateApiKeyRequest::new()
                    .name("renamed")
                    .status(WriteApiKeyStatus::Archived),
            )
            .await
            .unwrap();
    }
}
