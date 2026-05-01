//! Environments: container configuration for sessions.
//!
//! An environment defines the cloud container (pre-installed packages,
//! networking policy) where sessions run. Multiple sessions can share
//! one environment; each gets its own container instance.
//!
//! Environments are not versioned; mutate via re-create rather than
//! patch. The lifecycle is create → list/retrieve → archive → delete.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::MANAGED_AGENTS_BETA;

// =====================================================================
// Config types
// =====================================================================

/// Pre-installed packages, indexed by package manager. Caches across
/// sessions that share the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EnvironmentPackages {
    /// System packages (`apt-get`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apt: Vec<String>,
    /// Rust crates (`cargo install`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cargo: Vec<String>,
    /// Ruby gems.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gem: Vec<String>,
    /// Go modules (`go install`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub go: Vec<String>,
    /// Node packages (`npm install`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npm: Vec<String>,
    /// Python packages (`pip install`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pip: Vec<String>,
}

/// Networking policy for the container.
///
/// Forward-compatible: unknown wire `type` tags fall through to
/// [`Self::Other`] preserving the raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum Networking {
    /// Full outbound network access (default), except for a general
    /// safety blocklist.
    Unrestricted,
    /// Restrict to an explicit `allowed_hosts` list, optionally with
    /// MCP-server / package-manager bypass flags.
    Limited(LimitedNetworking),
    /// Unknown networking mode; raw JSON preserved.
    Other(serde_json::Value),
}

/// Body of a [`Networking::Limited`] policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LimitedNetworking {
    /// HTTPS-prefixed domains the container can reach.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_hosts: Vec<String>,
    /// Allow connections to MCP servers configured on the agent
    /// beyond `allowed_hosts`. Defaults to `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_mcp_servers: Option<bool>,
    /// Allow connections to public package registries (`PyPI`, `npm`,
    /// ...) beyond `allowed_hosts`. Defaults to `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_package_managers: Option<bool>,
}

const KNOWN_NETWORKING_TAGS: &[&str] = &["unrestricted", "limited"];

impl Serialize for Networking {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Unrestricted => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("type", "unrestricted")?;
                map.end()
            }
            Self::Limited(l) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "limited")?;
                if !l.allowed_hosts.is_empty() {
                    map.serialize_entry("allowed_hosts", &l.allowed_hosts)?;
                }
                if let Some(b) = l.allow_mcp_servers {
                    map.serialize_entry("allow_mcp_servers", &b)?;
                }
                if let Some(b) = l.allow_package_managers {
                    map.serialize_entry("allow_package_managers", &b)?;
                }
                map.end()
            }
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Networking {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let tag = raw.get("type").and_then(serde_json::Value::as_str);
        match tag {
            Some("unrestricted") if KNOWN_NETWORKING_TAGS.contains(&"unrestricted") => {
                Ok(Self::Unrestricted)
            }
            Some("limited") => {
                let l = serde_json::from_value::<LimitedNetworking>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::Limited(l))
            }
            _ => Ok(Self::Other(raw)),
        }
    }
}

/// Container configuration. Currently only the `cloud` shape is
/// documented; new shapes fall through to [`Self::Other`].
#[derive(Debug, Clone, PartialEq)]
pub enum EnvironmentConfig {
    /// Cloud container.
    Cloud(CloudConfig),
    /// Unknown config shape; raw JSON preserved.
    Other(serde_json::Value),
}

/// Body of an [`EnvironmentConfig::Cloud`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CloudConfig {
    /// Pre-installed packages.
    #[serde(default, skip_serializing_if = "is_default_packages")]
    pub packages: EnvironmentPackages,
    /// Networking policy. Defaults server-side to `unrestricted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub networking: Option<Networking>,
}

#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn is_default_packages(p: &EnvironmentPackages) -> bool {
    p.apt.is_empty()
        && p.cargo.is_empty()
        && p.gem.is_empty()
        && p.go.is_empty()
        && p.npm.is_empty()
        && p.pip.is_empty()
}

const KNOWN_ENV_CONFIG_TAGS: &[&str] = &["cloud"];

impl Serialize for EnvironmentConfig {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Cloud(c) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "cloud")?;
                if !is_default_packages(&c.packages) {
                    map.serialize_entry("packages", &c.packages)?;
                }
                if let Some(n) = &c.networking {
                    map.serialize_entry("networking", n)?;
                }
                map.end()
            }
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for EnvironmentConfig {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let tag = raw.get("type").and_then(serde_json::Value::as_str);
        match tag {
            Some("cloud") if KNOWN_ENV_CONFIG_TAGS.contains(&"cloud") => {
                let c =
                    serde_json::from_value::<CloudConfig>(raw).map_err(serde::de::Error::custom)?;
                Ok(Self::Cloud(c))
            }
            _ => Ok(Self::Other(raw)),
        }
    }
}

impl EnvironmentConfig {
    /// Build a cloud config with no packages and unrestricted networking.
    #[must_use]
    pub fn cloud() -> CloudConfigBuilder {
        CloudConfigBuilder::default()
    }
}

/// Builder for [`EnvironmentConfig::Cloud`].
#[derive(Debug, Default)]
pub struct CloudConfigBuilder {
    packages: EnvironmentPackages,
    networking: Option<Networking>,
}

impl CloudConfigBuilder {
    /// Add pip packages.
    #[must_use]
    pub fn pip<I, S>(mut self, packages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.packages.pip = packages.into_iter().map(Into::into).collect();
        self
    }

    /// Add npm packages.
    #[must_use]
    pub fn npm<I, S>(mut self, packages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.packages.npm = packages.into_iter().map(Into::into).collect();
        self
    }

    /// Add apt packages.
    #[must_use]
    pub fn apt<I, S>(mut self, packages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.packages.apt = packages.into_iter().map(Into::into).collect();
        self
    }

    /// Add cargo packages.
    #[must_use]
    pub fn cargo<I, S>(mut self, packages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.packages.cargo = packages.into_iter().map(Into::into).collect();
        self
    }

    /// Add gem packages.
    #[must_use]
    pub fn gem<I, S>(mut self, packages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.packages.gem = packages.into_iter().map(Into::into).collect();
        self
    }

    /// Add go modules.
    #[must_use]
    pub fn go<I, S>(mut self, packages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.packages.go = packages.into_iter().map(Into::into).collect();
        self
    }

    /// Set the networking policy.
    #[must_use]
    pub fn networking(mut self, networking: Networking) -> Self {
        self.networking = Some(networking);
        self
    }

    /// Finalize.
    #[must_use]
    pub fn build(self) -> EnvironmentConfig {
        EnvironmentConfig::Cloud(CloudConfig {
            packages: self.packages,
            networking: self.networking,
        })
    }
}

// =====================================================================
// Environment + request types
// =====================================================================

/// An environment definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Environment {
    /// Stable identifier (`env_...`).
    pub id: String,
    /// Wire type tag (`"environment"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Unique name within the workspace.
    pub name: String,
    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Free-form key-value metadata attached at create time.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Container configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<EnvironmentConfig>,
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

/// Request body for `POST /v1/environments`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateEnvironmentRequest {
    /// Unique name within the workspace.
    pub name: String,
    /// Container configuration.
    pub config: EnvironmentConfig,
}

impl CreateEnvironmentRequest {
    /// Build a request.
    #[must_use]
    pub fn new(name: impl Into<String>, config: EnvironmentConfig) -> Self {
        Self {
            name: name.into(),
            config,
        }
    }
}

/// Optional knobs for [`Environments::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListEnvironmentsParams {
    /// Pagination cursor.
    pub after: Option<String>,
    /// Pagination cursor.
    pub before: Option<String>,
    /// Page size.
    pub limit: Option<u32>,
    /// Whether to include archived environments.
    pub include_archived: Option<bool>,
}

impl ListEnvironmentsParams {
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
// Namespace handle
// =====================================================================

/// Namespace handle for the Environments API.
pub struct Environments<'a> {
    client: &'a Client,
}

/// Request body for [`Environments::update`]. All fields optional with
/// merge-patch semantics: omit a field to preserve.
///
/// `metadata` follows the same per-key delete protocol as
/// [`MetadataPatch`](super::agents::MetadataPatch).
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateEnvironmentRequest {
    /// Replacement name (1-256 chars).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Replacement description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Per-key metadata patch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<super::agents::MetadataPatch>,
    /// Replacement environment configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<EnvironmentConfig>,
}

impl UpdateEnvironmentRequest {
    /// Empty patch.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the new name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the new description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Apply a metadata patch.
    #[must_use]
    pub fn metadata(mut self, patch: super::agents::MetadataPatch) -> Self {
        self.metadata = Some(patch);
        self
    }

    /// Set the new config.
    #[must_use]
    pub fn config(mut self, config: EnvironmentConfig) -> Self {
        self.config = Some(config);
        self
    }
}

impl<'a> Environments<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/environments`.
    pub async fn create(&self, request: CreateEnvironmentRequest) -> Result<Environment> {
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/environments")
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/environments/{id}`.
    pub async fn retrieve(&self, environment_id: &str) -> Result<Environment> {
        let path = format!("/v1/environments/{environment_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/environments`.
    pub async fn list(&self, params: ListEnvironmentsParams) -> Result<Paginated<Environment>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/environments");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/environments/{id}`. Update an environment with
    /// merge-patch semantics; omitted fields are preserved.
    pub async fn update(
        &self,
        environment_id: &str,
        request: UpdateEnvironmentRequest,
    ) -> Result<Environment> {
        let path = format!("/v1/environments/{environment_id}");
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

    /// `POST /v1/environments/{id}/archive`. Read-only after archive;
    /// existing sessions continue.
    pub async fn archive(&self, environment_id: &str) -> Result<Environment> {
        let path = format!("/v1/environments/{environment_id}/archive");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `DELETE /v1/environments/{id}`. Only succeeds if no sessions
    /// reference the environment.
    pub async fn delete(&self, environment_id: &str) -> Result<()> {
        let path = format!("/v1/environments/{environment_id}");
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
    fn unrestricted_networking_serializes_minimal_object() {
        let v = serde_json::to_value(Networking::Unrestricted).unwrap();
        assert_eq!(v, json!({"type": "unrestricted"}));
    }

    #[test]
    fn limited_networking_round_trips_with_flags() {
        let n = Networking::Limited(LimitedNetworking {
            allowed_hosts: vec!["api.example.com".into()],
            allow_mcp_servers: Some(true),
            allow_package_managers: Some(false),
        });
        let v = serde_json::to_value(&n).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "limited",
                "allowed_hosts": ["api.example.com"],
                "allow_mcp_servers": true,
                "allow_package_managers": false
            })
        );
        let parsed: Networking = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn unknown_networking_falls_through_to_other() {
        let raw = json!({"type": "future_net", "x": 1});
        let parsed: Networking = serde_json::from_value(raw.clone()).unwrap();
        match parsed {
            Networking::Other(v) => assert_eq!(v, raw),
            Networking::Unrestricted | Networking::Limited(_) => panic!("expected Other"),
        }
    }

    #[test]
    fn cloud_config_serializes_with_packages_and_networking() {
        let cfg = EnvironmentConfig::cloud()
            .pip(["pandas", "numpy"])
            .npm(["express"])
            .networking(Networking::Limited(LimitedNetworking {
                allowed_hosts: vec!["api.example.com".into()],
                allow_mcp_servers: Some(true),
                allow_package_managers: Some(true),
            }))
            .build();
        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(v["type"], "cloud");
        assert_eq!(v["packages"]["pip"], json!(["pandas", "numpy"]));
        assert_eq!(v["packages"]["npm"], json!(["express"]));
        assert_eq!(v["networking"]["type"], "limited");
    }

    #[tokio::test]
    async fn create_environment_posts_full_payload() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/environments"))
            .and(body_partial_json(json!({
                "name": "python-dev",
                "config": {
                    "type": "cloud",
                    "networking": {"type": "unrestricted"}
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "env_01",
                "type": "environment",
                "name": "python-dev",
                "config": {"type": "cloud", "networking": {"type": "unrestricted"}}
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let env = client
            .managed_agents()
            .environments()
            .create(CreateEnvironmentRequest::new(
                "python-dev",
                EnvironmentConfig::cloud()
                    .networking(Networking::Unrestricted)
                    .build(),
            ))
            .await
            .unwrap();
        assert_eq!(env.id, "env_01");
        assert_eq!(env.name, "python-dev");
    }

    #[tokio::test]
    async fn list_environments_passes_include_archived_query() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/environments"))
            .and(wiremock::matchers::query_param("include_archived", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "env_01", "name": "python-dev"}],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .environments()
            .list(ListEnvironmentsParams {
                include_archived: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn archive_then_delete_environment() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/environments/env_01/archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "env_01",
                "name": "python-dev",
                "archived_at": "2026-04-30T12:00:00Z"
            })))
            .mount(&mock)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/v1/environments/env_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let env = client
            .managed_agents()
            .environments()
            .archive("env_01")
            .await
            .unwrap();
        assert!(env.archived_at.is_some());

        client
            .managed_agents()
            .environments()
            .delete("env_01")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn update_environment_posts_merge_patch() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/environments/env_42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "env_42",
                "type": "environment",
                "name": "renamed",
                "description": "new desc",
                "metadata": {"team": "data"},
                "config": {"type": "cloud"},
                "created_at": "2026-04-30T12:00:00Z",
                "updated_at": "2026-04-30T12:01:00Z"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let env = client
            .managed_agents()
            .environments()
            .update(
                "env_42",
                UpdateEnvironmentRequest::new()
                    .name("renamed")
                    .description("new desc")
                    .metadata(super::super::agents::MetadataPatch::new().set("team", "data")),
            )
            .await
            .unwrap();
        assert_eq!(env.name, "renamed");
        assert_eq!(env.description.as_deref(), Some("new desc"));
        assert_eq!(env.metadata.get("team").map(String::as_str), Some("data"));
    }
}
