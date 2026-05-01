//! The User Profiles API (beta).
//!
//! User profiles let your platform attach Anthropic-managed metadata
//! and trust grants to your end users. A [`UserProfile`] carries:
//!
//! - your platform's own [`external_id`](UserProfile::external_id)
//!   (e.g. your DB primary key) -- not enforced unique server-side,
//! - free-form [`metadata`](UserProfile::metadata) (≤16 keys),
//! - [`trust_grants`](UserProfile::trust_grants) the user has been
//!   granted (e.g. for elevated content categories), keyed by
//!   grant name with [`TrustGrantStatus`].
//!
//! Trust grants are not granted directly via this API. Instead, call
//! [`UserProfiles::create_enrollment_url`] to get a signed,
//! short-lived URL that you redirect the end user to so they can
//! complete the trust enrollment flow on Anthropic's side.
//!
//! # Beta
//!
//! Every method automatically sends
//! `anthropic-beta: user-profiles-2026-03-24`
//! ([`BetaHeader::UserProfiles`](crate::BetaHeader::UserProfiles)).
//! Override the beta version on the [`Client`](crate::Client) builder
//! if a newer revision is current.
//!
//! # Endpoints
//!
//! | Method | Path | Function |
//! |---|---|---|
//! | `POST` | `/v1/user_profiles` | [`UserProfiles::create`] |
//! | `GET` | `/v1/user_profiles` | [`UserProfiles::list`] |
//! | `GET` | `/v1/user_profiles/{user_profile_id}` | [`UserProfiles::get`] |
//! | `POST` | `/v1/user_profiles/{user_profile_id}` | [`UserProfiles::update`] |
//! | `POST` | `/v1/user_profiles/{user_profile_id}/enrollment_url` | [`UserProfiles::create_enrollment_url`] |

#![cfg(feature = "user-profiles")]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::PaginatedNextPage;

/// Beta version tag attached to every User Profiles request.
const USER_PROFILES_BETA: &[&str] = &["user-profiles-2026-03-24"];

// =====================================================================
// Wire types
// =====================================================================

/// Status of a single trust grant on a [`UserProfile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TrustGrantStatus {
    /// Grant is active and the user has been verified.
    Active,
    /// Enrollment has been started but not yet completed.
    Pending,
    /// Enrollment was attempted and rejected.
    Rejected,
}

/// One trust grant on a [`UserProfile`].
///
/// Grants live in [`UserProfile::trust_grants`] keyed by grant name
/// (e.g. `"cyber"`). The keying is open: new grant categories can
/// appear as Anthropic adds them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TrustGrant {
    /// Current grant status.
    pub status: TrustGrantStatus,
}

/// A user profile resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserProfile {
    /// Unique identifier (e.g. `uprof_011...`).
    pub id: String,
    /// Wire `type`; always `"user_profile"`.
    #[serde(rename = "type", default = "default_user_profile_kind")]
    pub kind: String,
    /// Platform's own identifier for this user (≤255 chars). Not
    /// enforced unique on the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Free-form key-value metadata (≤16 keys, key ≤64, value ≤512).
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Trust grants on this profile, keyed by grant name. The map is
    /// empty when no grants are active or in flight.
    #[serde(default)]
    pub trust_grants: HashMap<String, TrustGrant>,
    /// ISO-8601 (RFC 3339) creation timestamp.
    pub created_at: String,
    /// ISO-8601 (RFC 3339) last-update timestamp.
    pub updated_at: String,
}

fn default_user_profile_kind() -> String {
    "user_profile".to_owned()
}

/// A signed, short-lived enrollment URL returned by
/// [`UserProfiles::create_enrollment_url`].
///
/// Redirect the end user to [`Self::url`] before [`Self::expires_at`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EnrollmentUrl {
    /// Wire `type`; always `"enrollment_url"`.
    #[serde(rename = "type", default = "default_enrollment_url_kind")]
    pub kind: String,
    /// Enrollment URL to send to the end user.
    pub url: String,
    /// Expiry timestamp (RFC 3339). After this point the URL stops
    /// being valid; request a fresh one.
    pub expires_at: String,
}

fn default_enrollment_url_kind() -> String {
    "enrollment_url".to_owned()
}

// =====================================================================
// Request bodies
// =====================================================================

/// Body for [`UserProfiles::create`]. Both fields optional.
///
/// Use [`Self::new`] then chain [`Self::external_id`] and
/// [`Self::metadata`] / [`Self::metadata_entry`].
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct CreateUserProfileRequest {
    /// Platform's own identifier (≤255 chars).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Initial metadata (≤16 keys, key ≤64, value ≤512, non-empty).
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl CreateUserProfileRequest {
    /// Empty request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the external identifier.
    #[must_use]
    pub fn external_id(mut self, id: impl Into<String>) -> Self {
        self.external_id = Some(id.into());
        self
    }

    /// Replace the entire metadata map.
    #[must_use]
    pub fn metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Add or overwrite one metadata entry.
    #[must_use]
    pub fn metadata_entry(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Body for [`UserProfiles::update`].
///
/// **Merge semantics**: keys provided overwrite existing values; set a
/// key's value to an empty string to remove it; keys not provided are
/// left unchanged. `external_id`, when present, replaces the stored
/// value; omit to leave it unchanged.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateUserProfileRequest {
    /// Replacement external identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Metadata patch -- merged into the stored map.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl UpdateUserProfileRequest {
    /// Empty patch.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the stored `external_id`.
    #[must_use]
    pub fn external_id(mut self, id: impl Into<String>) -> Self {
        self.external_id = Some(id.into());
        self
    }

    /// Set or overwrite a metadata key.
    #[must_use]
    pub fn set_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Mark a metadata key for removal (sends `""` per the API
    /// contract).
    #[must_use]
    pub fn remove_metadata(mut self, key: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), String::new());
        self
    }
}

// =====================================================================
// List query params
// =====================================================================

/// Sort order for list endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ListOrder {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl ListOrder {
    fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

/// Query parameters for `GET /v1/user_profiles`.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListUserProfilesParams {
    /// Page size.
    pub limit: Option<u32>,
    /// Opaque cursor from a previous page's `next_page`.
    pub page: Option<String>,
    /// Sort order.
    pub order: Option<ListOrder>,
}

impl ListUserProfilesParams {
    /// Set the page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set the pagination cursor.
    #[must_use]
    pub fn page(mut self, cursor: impl Into<String>) -> Self {
        self.page = Some(cursor.into());
        self
    }

    /// Set the sort order.
    #[must_use]
    pub fn order(mut self, order: ListOrder) -> Self {
        self.order = Some(order);
        self
    }

    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        if let Some(o) = self.order {
            q.push(("order", o.as_str().to_owned()));
        }
        q
    }
}

// =====================================================================
// Namespace handle
// =====================================================================

/// Namespace handle for the User Profiles API.
///
/// Obtained via [`Client::user_profiles`].
pub struct UserProfiles<'a> {
    client: &'a Client,
}

impl<'a> UserProfiles<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/user_profiles`.
    pub async fn create(&self, request: CreateUserProfileRequest) -> Result<UserProfile> {
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/user_profiles")
                        .json(body)
                },
                USER_PROFILES_BETA,
            )
            .await
    }

    /// `GET /v1/user_profiles`.
    pub async fn list(
        &self,
        params: ListUserProfilesParams,
    ) -> Result<PaginatedNextPage<UserProfile>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/user_profiles");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                USER_PROFILES_BETA,
            )
            .await
    }

    /// `GET /v1/user_profiles/{user_profile_id}`.
    pub async fn get(&self, user_profile_id: &str) -> Result<UserProfile> {
        let path = format!("/v1/user_profiles/{user_profile_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                USER_PROFILES_BETA,
            )
            .await
    }

    /// `POST /v1/user_profiles/{user_profile_id}` -- update with merge
    /// semantics on `metadata`.
    pub async fn update(
        &self,
        user_profile_id: &str,
        request: UpdateUserProfileRequest,
    ) -> Result<UserProfile> {
        let path = format!("/v1/user_profiles/{user_profile_id}");
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(body)
                },
                USER_PROFILES_BETA,
            )
            .await
    }

    /// `POST /v1/user_profiles/{user_profile_id}/enrollment_url` --
    /// mint a short-lived URL the end user can visit to complete
    /// trust-grant enrollment.
    pub async fn create_enrollment_url(&self, user_profile_id: &str) -> Result<EnrollmentUrl> {
        let path = format!("/v1/user_profiles/{user_profile_id}/enrollment_url");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                USER_PROFILES_BETA,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{header_exists, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn user_profile_json(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "type": "user_profile",
            "external_id": "user_12345",
            "metadata": {"plan": "pro"},
            "trust_grants": {
                "cyber": {"status": "active"}
            },
            "created_at": "2026-03-15T10:00:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        })
    }

    #[tokio::test]
    async fn create_sends_optional_body_and_decodes_profile() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/user_profiles"))
            .and(header_exists("anthropic-beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_profile_json("uprof_C1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let p = client
            .user_profiles()
            .create(
                CreateUserProfileRequest::new()
                    .external_id("user_12345")
                    .metadata_entry("plan", "pro"),
            )
            .await
            .unwrap();
        assert_eq!(p.id, "uprof_C1");
        assert_eq!(p.external_id.as_deref(), Some("user_12345"));
        assert_eq!(p.metadata.get("plan").map(String::as_str), Some("pro"));
        assert_eq!(
            p.trust_grants.get("cyber").map(|g| g.status),
            Some(TrustGrantStatus::Active)
        );

        let recv = &mock.received_requests().await.unwrap()[0];
        let beta = recv
            .headers
            .get("anthropic-beta")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(beta.contains("user-profiles-2026-03-24"), "{beta}");
    }

    #[tokio::test]
    async fn create_omits_empty_metadata_from_request_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/user_profiles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_profile_json("uprof_C2")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let _ = client
            .user_profiles()
            .create(CreateUserProfileRequest::new())
            .await
            .unwrap();
        let recv = &mock.received_requests().await.unwrap()[0];
        let body: serde_json::Value = serde_json::from_slice(&recv.body).unwrap();
        // Empty body so the API uses defaults; both fields skipped.
        assert!(body.get("metadata").is_none(), "{body}");
        assert!(body.get("external_id").is_none(), "{body}");
    }

    #[tokio::test]
    async fn list_passes_limit_page_order_and_decodes_no_has_more() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/user_profiles"))
            .and(query_param("limit", "10"))
            .and(query_param("order", "desc"))
            .and(query_param("page", "page_X"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [user_profile_json("uprof_L1")],
                "next_page": "page_Y"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .user_profiles()
            .list(
                ListUserProfilesParams::default()
                    .limit(10)
                    .page("page_X")
                    .order(ListOrder::Desc),
            )
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.next_cursor(), Some("page_Y"));
        // List response omits `has_more`; envelope should default to false.
        assert!(!page.has_more);
    }

    #[tokio::test]
    async fn get_decodes_single_profile() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/user_profiles/uprof_G1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_profile_json("uprof_G1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let p = client.user_profiles().get("uprof_G1").await.unwrap();
        assert_eq!(p.id, "uprof_G1");
        assert_eq!(p.kind, "user_profile");
    }

    #[tokio::test]
    async fn update_sends_metadata_merge_patch_with_empty_string_for_deletion() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/user_profiles/uprof_U1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_profile_json("uprof_U1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let _ = client
            .user_profiles()
            .update(
                "uprof_U1",
                UpdateUserProfileRequest::new()
                    .set_metadata("plan", "enterprise")
                    .remove_metadata("legacy_flag"),
            )
            .await
            .unwrap();

        let recv = &mock.received_requests().await.unwrap()[0];
        let body: serde_json::Value = serde_json::from_slice(&recv.body).unwrap();
        assert_eq!(body["metadata"]["plan"], "enterprise");
        // Removal is encoded as an empty string per the API contract.
        assert_eq!(body["metadata"]["legacy_flag"], "");
    }

    #[tokio::test]
    async fn create_enrollment_url_returns_signed_url_and_expiry() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/user_profiles/uprof_E1/enrollment_url"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "type": "enrollment_url",
                "url": "https://platform.claude.com/user-profiles/enrollment/abc123",
                "expires_at": "2026-03-15T10:15:00Z"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let url = client
            .user_profiles()
            .create_enrollment_url("uprof_E1")
            .await
            .unwrap();
        assert!(url.url.contains("enrollment/abc123"));
        assert_eq!(url.expires_at, "2026-03-15T10:15:00Z");
        assert_eq!(url.kind, "enrollment_url");
    }

    #[test]
    fn trust_grant_status_round_trips_known_values() {
        let g: TrustGrant = serde_json::from_value(json!({"status": "pending"})).unwrap();
        assert_eq!(g.status, TrustGrantStatus::Pending);
        let g: TrustGrant = serde_json::from_value(json!({"status": "rejected"})).unwrap();
        assert_eq!(g.status, TrustGrantStatus::Rejected);
        let json = serde_json::to_value(TrustGrant {
            status: TrustGrantStatus::Active,
        })
        .unwrap();
        assert_eq!(json, json!({"status": "active"}));
    }

    #[test]
    fn list_order_serializes_as_lowercase() {
        assert_eq!(ListOrder::Asc.as_str(), "asc");
        assert_eq!(ListOrder::Desc.as_str(), "desc");
        let v = serde_json::to_value(ListOrder::Desc).unwrap();
        assert_eq!(v, json!("desc"));
    }

    #[test]
    fn user_profile_tolerates_missing_optional_fields() {
        // Server may return a profile with no external_id, no metadata,
        // and no trust_grants.
        let raw = json!({
            "id": "uprof_M1",
            "type": "user_profile",
            "created_at": "2026-03-15T10:00:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        });
        let p: UserProfile = serde_json::from_value(raw).unwrap();
        assert_eq!(p.id, "uprof_M1");
        assert!(p.external_id.is_none());
        assert!(p.metadata.is_empty());
        assert!(p.trust_grants.is_empty());
    }
}
