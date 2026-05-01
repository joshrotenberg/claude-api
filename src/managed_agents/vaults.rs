//! Vaults: per-user collections of MCP credentials.
//!
//! Vaults are workspace-scoped and reference credentials by ID at session
//! creation time. Credentials are write-only: secret fields are never
//! returned in API responses.
//!
//! Constraints:
//! - One active credential per `mcp_server_url` per vault.
//! - `mcp_server_url` is immutable after creation; archive and replace.
//! - Maximum 20 credentials per vault.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::MANAGED_AGENTS_BETA;

// =====================================================================
// Vault types
// =====================================================================

/// A vault: collection of MCP credentials, typically scoped to one
/// end-user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Vault {
    /// Stable identifier (`vlt_...`).
    pub id: String,
    /// Wire type tag (always `"vault"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Human-readable display name.
    pub display_name: String,
    /// Free-form metadata for mapping back to caller-side records.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Last-modified timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Set when archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Request body for `POST /v1/vaults`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateVaultRequest {
    /// Required.
    pub display_name: String,
    /// Optional caller-side metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl CreateVaultRequest {
    /// Build a request with the given display name.
    #[must_use]
    pub fn new(display_name: impl Into<String>) -> Self {
        Self {
            display_name: display_name.into(),
            metadata: HashMap::new(),
        }
    }

    /// Attach a metadata entry.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Optional knobs for [`Vaults::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListVaultsParams {
    /// Pagination cursor.
    pub after: Option<String>,
    /// Pagination cursor.
    pub before: Option<String>,
    /// Page size.
    pub limit: Option<u32>,
    /// Whether to include archived vaults.
    pub include_archived: Option<bool>,
}

impl ListVaultsParams {
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
// Credential types
// =====================================================================

/// Token-endpoint authentication scheme for refreshing OAuth credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TokenEndpointAuth {
    /// Public client; no credential sent on refresh.
    None,
    /// HTTP Basic auth with the client secret.
    ClientSecretBasic {
        /// Client secret (write-only).
        client_secret: String,
    },
    /// Client secret sent in the POST body.
    ClientSecretPost {
        /// Client secret (write-only).
        client_secret: String,
    },
}

/// OAuth refresh configuration on an `mcp_oauth` credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OAuthRefresh {
    /// OAuth token endpoint URL.
    pub token_endpoint: String,
    /// Client ID registered with the OAuth provider.
    pub client_id: String,
    /// Refresh token (write-only).
    pub refresh_token: String,
    /// Token-endpoint authentication scheme.
    pub token_endpoint_auth: TokenEndpointAuth,
    /// Optional scope string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Credential authentication payload.
///
/// Forward-compatible: unknown wire `type` tags fall through to
/// [`Self::Other`] preserving the raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum CredentialAuth {
    /// MCP OAuth token, optionally with a refresh block. Anthropic
    /// refreshes on your behalf when `refresh` is configured.
    McpOauth(McpOauthAuth),
    /// Static bearer token (API key, PAT). No refresh flow.
    StaticBearer(StaticBearerAuth),
    /// Unknown auth type; raw JSON preserved.
    Other(serde_json::Value),
}

/// `mcp_oauth` credential body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct McpOauthAuth {
    /// MCP server endpoint this credential authenticates against.
    /// Immutable after creation.
    pub mcp_server_url: String,
    /// Access token (write-only).
    pub access_token: String,
    /// Token expiration (RFC3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Refresh configuration; if present, Anthropic refreshes on your
    /// behalf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<OAuthRefresh>,
}

/// `static_bearer` credential body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StaticBearerAuth {
    /// MCP server endpoint. Immutable after creation.
    pub mcp_server_url: String,
    /// Bearer token (write-only).
    pub token: String,
}

const KNOWN_CREDENTIAL_AUTH_TAGS: &[&str] = &["mcp_oauth", "static_bearer"];

impl Serialize for CredentialAuth {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::McpOauth(v) => {
                use serde::ser::SerializeMap;
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "mcp_oauth")?;
                map.serialize_entry("mcp_server_url", &v.mcp_server_url)?;
                map.serialize_entry("access_token", &v.access_token)?;
                if let Some(e) = &v.expires_at {
                    map.serialize_entry("expires_at", e)?;
                }
                if let Some(r) = &v.refresh {
                    map.serialize_entry("refresh", r)?;
                }
                map.end()
            }
            Self::StaticBearer(v) => {
                use serde::ser::SerializeMap;
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "static_bearer")?;
                map.serialize_entry("mcp_server_url", &v.mcp_server_url)?;
                map.serialize_entry("token", &v.token)?;
                map.end()
            }
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for CredentialAuth {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let tag = raw.get("type").and_then(serde_json::Value::as_str);
        match tag {
            Some("mcp_oauth") if KNOWN_CREDENTIAL_AUTH_TAGS.contains(&"mcp_oauth") => {
                let body = serde_json::from_value::<McpOauthAuth>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::McpOauth(body))
            }
            Some("static_bearer") if KNOWN_CREDENTIAL_AUTH_TAGS.contains(&"static_bearer") => {
                let body = serde_json::from_value::<StaticBearerAuth>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::StaticBearer(body))
            }
            _ => Ok(Self::Other(raw)),
        }
    }
}

impl CredentialAuth {
    /// Build an [`CredentialAuth::McpOauth`] credential.
    #[must_use]
    pub fn mcp_oauth(
        mcp_server_url: impl Into<String>,
        access_token: impl Into<String>,
    ) -> McpOauthBuilder {
        McpOauthBuilder {
            mcp_server_url: mcp_server_url.into(),
            access_token: access_token.into(),
            expires_at: None,
            refresh: None,
        }
    }

    /// Build a [`CredentialAuth::StaticBearer`] credential.
    #[must_use]
    pub fn static_bearer(mcp_server_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self::StaticBearer(StaticBearerAuth {
            mcp_server_url: mcp_server_url.into(),
            token: token.into(),
        })
    }
}

/// Builder for [`McpOauthAuth`] credentials.
#[derive(Debug, Clone)]
pub struct McpOauthBuilder {
    mcp_server_url: String,
    access_token: String,
    expires_at: Option<String>,
    refresh: Option<OAuthRefresh>,
}

impl McpOauthBuilder {
    /// Set token expiration (RFC3339).
    #[must_use]
    pub fn expires_at(mut self, when: impl Into<String>) -> Self {
        self.expires_at = Some(when.into());
        self
    }

    /// Attach refresh configuration.
    #[must_use]
    pub fn refresh(mut self, refresh: OAuthRefresh) -> Self {
        self.refresh = Some(refresh);
        self
    }

    /// Finalize as a [`CredentialAuth::McpOauth`].
    #[must_use]
    pub fn build(self) -> CredentialAuth {
        CredentialAuth::McpOauth(McpOauthAuth {
            mcp_server_url: self.mcp_server_url,
            access_token: self.access_token,
            expires_at: self.expires_at,
            refresh: self.refresh,
        })
    }
}

/// A stored credential. Secret fields are never echoed in API
/// responses; the [`auth`](Self::auth) object carries only the
/// non-secret metadata (server URL, expiry, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Credential {
    /// Stable identifier (`cred_...`).
    pub id: String,
    /// Wire type tag (`"credential"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Parent vault ID.
    pub vault_id: String,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Free-form metadata for mapping back to caller-side records.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Auth shape with non-secret fields populated. `None` if the
    /// server doesn't return an auth block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<CredentialAuthResponse>,
    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Last-modified timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Set when the credential is archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Auth payload as returned on a credential **response**. Mirrors
/// [`CredentialAuth`] but never carries the secret token fields.
///
/// Forward-compatible: unknown wire `type` tags fall through to
/// [`Self::Other`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CredentialAuthResponse {
    /// MCP OAuth credential metadata (no token).
    McpOauth {
        /// MCP server URL.
        mcp_server_url: String,
        /// Token expiration (RFC3339).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<String>,
        /// Refresh configuration, when configured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh: Option<serde_json::Value>,
    },
    /// Static-bearer credential metadata (no token).
    StaticBearer {
        /// MCP server URL.
        mcp_server_url: String,
    },
    /// Forward-compat fallback for unknown auth `type` values.
    #[serde(other)]
    Other,
}

/// Request body for `POST /v1/vaults/{id}/credentials`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateCredentialRequest {
    /// Auth payload (write-only secrets).
    pub auth: CredentialAuth,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl CreateCredentialRequest {
    /// Build a credential-creation request with the given auth payload.
    #[must_use]
    pub fn new(auth: CredentialAuth) -> Self {
        Self {
            auth,
            display_name: None,
        }
    }

    /// Attach a display name.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }
}

/// Patch applied to an existing credential. Only the secret payload and
/// a few metadata fields are mutable; `mcp_server_url`, `token_endpoint`,
/// and `client_id` are locked after creation.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateCredentialRequest {
    /// New auth payload. Use [`CredentialAuthPatch`] for partial
    /// updates that don't replace the entire shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<CredentialAuthPatch>,
    /// New display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Partial auth update for [`UpdateCredentialRequest`]. Pass only the
/// fields you want to rotate; immutable fields (`mcp_server_url`,
/// `token_endpoint`, `client_id`) cannot be changed.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CredentialAuthPatch {
    /// Rotate an `mcp_oauth` credential's tokens / expiry.
    McpOauth {
        /// New access token.
        #[serde(skip_serializing_if = "Option::is_none")]
        access_token: Option<String>,
        /// New expiry.
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_at: Option<String>,
        /// Refresh-block patch (e.g. new `refresh_token`).
        #[serde(skip_serializing_if = "Option::is_none")]
        refresh: Option<OAuthRefreshPatch>,
    },
    /// Rotate a `static_bearer` credential's token.
    StaticBearer {
        /// New token.
        #[serde(skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
}

/// Partial refresh-block patch for [`CredentialAuthPatch::McpOauth`].
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct OAuthRefreshPatch {
    /// New refresh token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// New scope string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// New token-endpoint auth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth: Option<TokenEndpointAuth>,
}

// =====================================================================
// Namespace handles
// =====================================================================

/// Namespace handle for the Vaults API.
pub struct Vaults<'a> {
    client: &'a Client,
}

impl<'a> Vaults<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/vaults`.
    pub async fn create(&self, request: CreateVaultRequest) -> Result<Vault> {
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/vaults")
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/vaults/{id}`.
    pub async fn retrieve(&self, vault_id: &str) -> Result<Vault> {
        let path = format!("/v1/vaults/{vault_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/vaults`.
    pub async fn list(&self, params: ListVaultsParams) -> Result<Paginated<Vault>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/vaults");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/vaults/{id}/archive`.
    pub async fn archive(&self, vault_id: &str) -> Result<Vault> {
        let path = format!("/v1/vaults/{vault_id}/archive");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `DELETE /v1/vaults/{id}`. Hard delete; no audit trail. Use
    /// archive if you need one.
    pub async fn delete(&self, vault_id: &str) -> Result<()> {
        let path = format!("/v1/vaults/{vault_id}");
        let _: serde_json::Value = self
            .client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await?;
        Ok(())
    }

    /// Sub-namespace for credential operations on a single vault.
    #[must_use]
    pub fn credentials(&self, vault_id: impl Into<String>) -> Credentials<'_> {
        Credentials {
            client: self.client,
            vault_id: vault_id.into(),
        }
    }
}

/// Namespace handle for credential operations on a single vault.
pub struct Credentials<'a> {
    client: &'a Client,
    vault_id: String,
}

impl Credentials<'_> {
    /// `POST /v1/vaults/{vault_id}/credentials`.
    pub async fn create(&self, request: CreateCredentialRequest) -> Result<Credential> {
        let path = format!("/v1/vaults/{}/credentials", self.vault_id);
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

    /// `POST /v1/vaults/{vault_id}/credentials/{cred_id}` (update;
    /// rotate tokens, change display name).
    pub async fn update(
        &self,
        credential_id: &str,
        request: UpdateCredentialRequest,
    ) -> Result<Credential> {
        let path = format!("/v1/vaults/{}/credentials/{credential_id}", self.vault_id);
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

    /// `GET /v1/vaults/{vault_id}/credentials/{cred_id}`.
    pub async fn retrieve(&self, credential_id: &str) -> Result<Credential> {
        let path = format!("/v1/vaults/{}/credentials/{credential_id}", self.vault_id);
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/vaults/{vault_id}/credentials`.
    pub async fn list(&self, params: ListVaultsParams) -> Result<Paginated<Credential>> {
        let path = format!("/v1/vaults/{}/credentials", self.vault_id);
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

    /// `POST /v1/vaults/{vault_id}/credentials/{cred_id}/archive`.
    /// Frees the `mcp_server_url` slot for a replacement credential.
    pub async fn archive(&self, credential_id: &str) -> Result<Credential> {
        let path = format!(
            "/v1/vaults/{}/credentials/{credential_id}/archive",
            self.vault_id
        );
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `DELETE /v1/vaults/{vault_id}/credentials/{cred_id}`.
    pub async fn delete(&self, credential_id: &str) -> Result<()> {
        let path = format!("/v1/vaults/{}/credentials/{credential_id}", self.vault_id);
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
    fn mcp_oauth_round_trips_via_credential_auth() {
        let auth = CredentialAuth::mcp_oauth("https://mcp.slack.com/mcp", "xoxp-token")
            .expires_at("2026-04-15T00:00:00Z")
            .refresh(OAuthRefresh {
                token_endpoint: "https://slack.com/api/oauth.v2.access".into(),
                client_id: "1234567890".into(),
                refresh_token: "xoxe-refresh".into(),
                token_endpoint_auth: TokenEndpointAuth::ClientSecretPost {
                    client_secret: "abc123".into(),
                },
                scope: Some("channels:read".into()),
            })
            .build();
        let v = serde_json::to_value(&auth).unwrap();
        assert_eq!(v["type"], "mcp_oauth");
        assert_eq!(v["mcp_server_url"], "https://mcp.slack.com/mcp");
        assert_eq!(v["access_token"], "xoxp-token");
        assert_eq!(v["refresh"]["client_id"], "1234567890");
        assert_eq!(
            v["refresh"]["token_endpoint_auth"]["type"],
            "client_secret_post"
        );
    }

    #[test]
    fn static_bearer_round_trips() {
        let auth = CredentialAuth::static_bearer("https://mcp.linear.app/mcp", "lin_api_x");
        let v = serde_json::to_value(&auth).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "static_bearer",
                "mcp_server_url": "https://mcp.linear.app/mcp",
                "token": "lin_api_x"
            })
        );
    }

    #[test]
    fn unknown_auth_type_falls_through_to_other() {
        let raw = json!({"type": "future_auth", "x": 1});
        let parsed: CredentialAuth = serde_json::from_value(raw.clone()).unwrap();
        match parsed {
            CredentialAuth::Other(v) => assert_eq!(v, raw),
            CredentialAuth::McpOauth(_) | CredentialAuth::StaticBearer(_) => {
                panic!("expected Other")
            }
        }
    }

    #[tokio::test]
    async fn create_vault_posts_with_metadata() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/vaults"))
            .and(body_partial_json(json!({
                "display_name": "Alice",
                "metadata": {"external_user_id": "usr_abc"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "vlt_01",
                "type": "vault",
                "display_name": "Alice",
                "metadata": {"external_user_id": "usr_abc"},
                "created_at": "2026-04-30T12:00:00Z"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateVaultRequest::new("Alice").with_metadata("external_user_id", "usr_abc");
        let v = client.managed_agents().vaults().create(req).await.unwrap();
        assert_eq!(v.id, "vlt_01");
        assert_eq!(
            v.metadata.get("external_user_id").map(String::as_str),
            Some("usr_abc")
        );
    }

    #[tokio::test]
    async fn create_credential_serializes_static_bearer_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/vaults/vlt_01/credentials"))
            .and(body_partial_json(json!({
                "auth": {
                    "type": "static_bearer",
                    "mcp_server_url": "https://mcp.linear.app/mcp",
                    "token": "lin_api_x"
                },
                "display_name": "Linear API key"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cred_01",
                "type": "credential",
                "vault_id": "vlt_01",
                "auth": {
                    "type": "static_bearer",
                    "mcp_server_url": "https://mcp.linear.app/mcp"
                }
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateCredentialRequest::new(CredentialAuth::static_bearer(
            "https://mcp.linear.app/mcp",
            "lin_api_x",
        ))
        .with_display_name("Linear API key");
        let c = client
            .managed_agents()
            .vaults()
            .credentials("vlt_01")
            .create(req)
            .await
            .unwrap();
        assert_eq!(c.id, "cred_01");
    }

    #[tokio::test]
    async fn update_credential_rotates_token_via_patch() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/vaults/vlt_01/credentials/cred_01"))
            .and(body_partial_json(json!({
                "auth": {
                    "type": "mcp_oauth",
                    "access_token": "xoxp-new",
                    "refresh": {"refresh_token": "xoxe-new"}
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cred_01",
                "type": "credential",
                "vault_id": "vlt_01",
                "auth": {
                    "type": "mcp_oauth",
                    "mcp_server_url": "https://mcp.slack.com/mcp"
                }
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let patch = UpdateCredentialRequest {
            auth: Some(CredentialAuthPatch::McpOauth {
                access_token: Some("xoxp-new".into()),
                expires_at: None,
                refresh: Some(OAuthRefreshPatch {
                    refresh_token: Some("xoxe-new".into()),
                    ..Default::default()
                }),
            }),
            display_name: None,
        };
        client
            .managed_agents()
            .vaults()
            .credentials("vlt_01")
            .update("cred_01", patch)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retrieve_vault_returns_typed_record() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/vaults/vlt_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "vlt_01",
                "type": "vault",
                "display_name": "Alice"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let v = client
            .managed_agents()
            .vaults()
            .retrieve("vlt_01")
            .await
            .unwrap();
        assert_eq!(v.id, "vlt_01");
    }

    #[tokio::test]
    async fn list_vaults_passes_pagination_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/vaults"))
            .and(wiremock::matchers::query_param("limit", "10"))
            .and(wiremock::matchers::query_param("after", "vlt_x"))
            .and(wiremock::matchers::query_param("include_archived", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "vlt_01", "display_name": "Alice"}],
                "has_more": false
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .vaults()
            .list(ListVaultsParams {
                after: Some("vlt_x".into()),
                limit: Some(10),
                include_archived: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn archive_vault_posts_to_archive_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/vaults/vlt_01/archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "vlt_01",
                "display_name": "Alice",
                "archived_at": "2026-04-30T12:00:00Z"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let v = client
            .managed_agents()
            .vaults()
            .archive("vlt_01")
            .await
            .unwrap();
        assert!(v.archived_at.is_some());
    }

    #[tokio::test]
    async fn delete_vault_returns_unit() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/vaults/vlt_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .managed_agents()
            .vaults()
            .delete("vlt_01")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retrieve_credential_returns_record_without_secrets() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/vaults/vlt_01/credentials/cred_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cred_01",
                "type": "credential",
                "vault_id": "vlt_01",
                "display_name": "Linear",
                "auth": {
                    "type": "static_bearer",
                    "mcp_server_url": "https://mcp.linear.app/mcp"
                }
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let c = client
            .managed_agents()
            .vaults()
            .credentials("vlt_01")
            .retrieve("cred_01")
            .await
            .unwrap();
        assert_eq!(c.vault_id, "vlt_01");
        match c.auth.unwrap() {
            CredentialAuthResponse::StaticBearer { mcp_server_url } => {
                assert_eq!(mcp_server_url, "https://mcp.linear.app/mcp");
            }
            _ => panic!("expected StaticBearer auth"),
        }
    }

    #[tokio::test]
    async fn list_credentials_paginates_under_vault() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/vaults/vlt_01/credentials"))
            .and(wiremock::matchers::query_param("limit", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "cred_01", "vault_id": "vlt_01"}],
                "has_more": false
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .vaults()
            .credentials("vlt_01")
            .list(ListVaultsParams {
                limit: Some(5),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn delete_credential_returns_unit() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/vaults/vlt_01/credentials/cred_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        client
            .managed_agents()
            .vaults()
            .credentials("vlt_01")
            .delete("cred_01")
            .await
            .unwrap();
    }

    #[test]
    fn token_endpoint_auth_round_trips_all_three_variants() {
        for auth in [
            TokenEndpointAuth::None,
            TokenEndpointAuth::ClientSecretBasic {
                client_secret: "abc".into(),
            },
            TokenEndpointAuth::ClientSecretPost {
                client_secret: "def".into(),
            },
        ] {
            let v = serde_json::to_value(&auth).unwrap();
            let parsed: TokenEndpointAuth = serde_json::from_value(v).unwrap();
            assert_eq!(parsed, auth);
        }
    }

    #[tokio::test]
    async fn archive_credential_posts_to_archive_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/vaults/vlt_01/credentials/cred_01/archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cred_01",
                "type": "credential",
                "vault_id": "vlt_01",
                "archived_at": "2026-04-30T12:00:00Z"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let c = client
            .managed_agents()
            .vaults()
            .credentials("vlt_01")
            .archive("cred_01")
            .await
            .unwrap();
        assert!(c.archived_at.is_some());
    }
}
