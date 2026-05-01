//! The Skills API (beta).
//!
//! Skills are reusable named bundles of files (with a required
//! `SKILL.md` at the bundle root) that Claude can load on demand. A
//! [`Skill`] is the named container and a [`SkillVersion`] is one
//! immutable snapshot of its files.
//!
//! # Beta
//!
//! Every Skills method automatically sends
//! `anthropic-beta: skills-2025-10-02`. Override the beta version on
//! the [`Client`] builder if a newer revision is current.
//!
//! # Endpoints
//!
//! | Method | Path | Function |
//! |---|---|---|
//! | `POST` | `/v1/skills` | [`Skills::create`] |
//! | `GET` | `/v1/skills` | [`Skills::list`] |
//! | `GET` | `/v1/skills/{skill_id}` | [`Skills::get`] |
//! | `DELETE` | `/v1/skills/{skill_id}` | [`Skills::delete`] |
//! | `POST` | `/v1/skills/{skill_id}/versions` | [`Skills::create_version`] |
//! | `GET` | `/v1/skills/{skill_id}/versions` | [`Skills::list_versions`] |
//! | `GET` | `/v1/skills/{skill_id}/versions/{version}` | [`Skills::get_version`] |
//! | `DELETE` | `/v1/skills/{skill_id}/versions/{version}` | [`Skills::delete_version`] |

#![cfg(feature = "skills")]

use std::path::Path;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{Error, Result};
use crate::pagination::PaginatedNextPage;

/// Beta version tag attached to every Skills API request.
const SKILLS_BETA: &[&str] = &["skills-2025-10-02"];

// =====================================================================
// Wire types
// =====================================================================

/// Source of a skill (who created it). Open-string enum: unknown
/// values fall through to [`Self::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    /// Created by a user in the calling organization.
    Custom,
    /// Built-in skill maintained by Anthropic.
    Anthropic,
    /// Forward-compat fallback for unknown source values.
    Other(String),
}

impl Serialize for SkillSource {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            Self::Custom => "custom",
            Self::Anthropic => "anthropic",
            Self::Other(v) => v,
        })
    }
}

impl<'de> Deserialize<'de> for SkillSource {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "custom" => Self::Custom,
            "anthropic" => Self::Anthropic,
            _ => Self::Other(s),
        })
    }
}

/// Subset of [`SkillSource`] valid as the `source` query filter on
/// `GET /v1/skills`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillSourceFilter {
    /// Filter to user-created skills.
    Custom,
    /// Filter to Anthropic-provided skills.
    Anthropic,
}

impl SkillSourceFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::Custom => "custom",
            Self::Anthropic => "anthropic",
        }
    }
}

/// A skill resource. The `latest_version` field points to a
/// [`SkillVersion::version`] string -- pass it to
/// [`Skills::get_version`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Skill {
    /// Unique identifier (e.g. `skill_01JABC...`). The format may
    /// change over time.
    pub id: String,
    /// Wire `type`; always `"skill"`.
    #[serde(rename = "type", default = "default_skill_kind")]
    pub kind: String,
    /// Human-readable label (not sent to the model in the prompt).
    pub display_title: String,
    /// Most recent version timestamp string (e.g. `"1759178010641129"`).
    pub latest_version: String,
    /// Who created the skill.
    pub source: SkillSource,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
    /// ISO-8601 last-update timestamp.
    pub updated_at: String,
}

fn default_skill_kind() -> String {
    "skill".to_owned()
}

/// One immutable snapshot of a [`Skill`]'s file bundle.
///
/// `name` and `description` are extracted from the bundle's `SKILL.md`
/// at upload time; `directory` is the top-level directory name in the
/// upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SkillVersion {
    /// Unique identifier for the version (e.g. `skillver_01JABC...`).
    pub id: String,
    /// Wire `type`; always `"skill_version"`.
    #[serde(rename = "type", default = "default_skill_version_kind")]
    pub kind: String,
    /// Parent skill ID.
    pub skill_id: String,
    /// Version identifier; a Unix-epoch-millis string
    /// (e.g. `"1759178010641129"`). Pass this to
    /// [`Skills::get_version`] / [`Skills::delete_version`].
    pub version: String,
    /// Human-readable name from the uploaded `SKILL.md` front-matter.
    pub name: String,
    /// Description from the uploaded `SKILL.md`.
    pub description: String,
    /// Top-level directory name extracted from the uploaded files.
    pub directory: String,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

fn default_skill_version_kind() -> String {
    "skill_version".to_owned()
}

/// Confirmation returned by `DELETE /v1/skills/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SkillDeleted {
    /// ID of the deleted skill.
    pub id: String,
    /// Wire `type`; typically `"skill_deleted"`.
    #[serde(rename = "type", default)]
    pub kind: String,
}

/// Confirmation returned by `DELETE /v1/skills/{id}/versions/{version}`.
///
/// Note: unlike [`SkillDeleted`], `id` here is the *version timestamp
/// string* (e.g. `"1759178010641129"`), not a `skillver_*` identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SkillVersionDeleted {
    /// Version timestamp of the deleted skill version.
    pub id: String,
    /// Wire `type`; typically `"skill_version_deleted"`.
    #[serde(rename = "type", default)]
    pub kind: String,
}

// =====================================================================
// Multipart upload helpers
// =====================================================================

/// One file in a skill upload. The `path` is the relative path inside
/// the bundle and **must include the top-level directory**, e.g.
/// `"my-skill/SKILL.md"`. Every upload must contain a `SKILL.md` at
/// the directory root.
#[derive(Debug, Clone)]
pub struct SkillFile {
    /// Relative path inside the bundle, including the top-level
    /// directory (e.g. `"my-skill/SKILL.md"`).
    pub path: String,
    /// Raw file bytes.
    pub contents: Bytes,
}

impl SkillFile {
    /// Build from a path string + raw bytes.
    #[must_use]
    pub fn new(path: impl Into<String>, contents: impl Into<Bytes>) -> Self {
        Self {
            path: path.into(),
            contents: contents.into(),
        }
    }

    /// Read a file from disk. The bundle path is the file's name
    /// component; for nested layouts, build [`SkillFile`] manually with
    /// the full relative path.
    pub async fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bundle_path = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                Error::InvalidConfig(format!("invalid filename in path {}", path.display()))
            })?
            .to_owned();
        let contents = tokio::fs::read(path).await?;
        Ok(Self {
            path: bundle_path,
            contents: Bytes::from(contents),
        })
    }
}

/// Body for [`Skills::create`].
///
/// Use [`Self::new`] then chain [`Self::display_title`] and
/// [`Self::file`] / [`Self::files`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct CreateSkillRequest {
    /// Human-readable label (not sent to the model).
    pub display_title: Option<String>,
    /// Files to upload. Must include a `SKILL.md` at the bundle root.
    pub files: Vec<SkillFile>,
}

impl CreateSkillRequest {
    /// Empty request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the display title.
    #[must_use]
    pub fn display_title(mut self, t: impl Into<String>) -> Self {
        self.display_title = Some(t.into());
        self
    }

    /// Append one file to the upload.
    #[must_use]
    pub fn file(mut self, f: SkillFile) -> Self {
        self.files.push(f);
        self
    }

    /// Append many files.
    #[must_use]
    pub fn files(mut self, fs: impl IntoIterator<Item = SkillFile>) -> Self {
        self.files.extend(fs);
        self
    }
}

fn build_form(display_title: Option<&str>, files: &[SkillFile]) -> reqwest::multipart::Form {
    let mut form = reqwest::multipart::Form::new();
    if let Some(t) = display_title {
        form = form.text("display_title", t.to_owned());
    }
    for f in files {
        let part = reqwest::multipart::Part::bytes(f.contents.to_vec()).file_name(f.path.clone());
        form = form.part("files", part);
    }
    form
}

// =====================================================================
// List query params
// =====================================================================

/// Query parameters for `GET /v1/skills`.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListSkillsParams {
    /// Page size (1..=100, default 20).
    pub limit: Option<u32>,
    /// Opaque cursor from a previous page's `next_page`.
    pub page: Option<String>,
    /// Filter by source.
    pub source: Option<SkillSourceFilter>,
}

impl ListSkillsParams {
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

    /// Filter by source.
    #[must_use]
    pub fn source(mut self, source: SkillSourceFilter) -> Self {
        self.source = Some(source);
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
        if let Some(s) = self.source {
            q.push(("source", s.as_str().to_owned()));
        }
        q
    }
}

/// Query parameters for `GET /v1/skills/{skill_id}/versions`.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListSkillVersionsParams {
    /// Page size (1..=1000, default 20).
    pub limit: Option<u32>,
    /// Opaque cursor from a previous page's `next_page`.
    pub page: Option<String>,
}

impl ListSkillVersionsParams {
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

    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        q
    }
}

// =====================================================================
// Namespace handle
// =====================================================================

/// Namespace handle for the Skills API.
///
/// Obtained via [`Client::skills`].
pub struct Skills<'a> {
    client: &'a Client,
}

impl<'a> Skills<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/skills` -- create a new skill from a multipart upload.
    ///
    /// Multipart bodies are single-use; this method does not retry.
    pub async fn create(&self, request: CreateSkillRequest) -> Result<Skill> {
        let form = build_form(request.display_title.as_deref(), &request.files);
        let builder = self
            .client
            .request_builder(reqwest::Method::POST, "/v1/skills")
            .multipart(form);
        self.client.execute(builder, SKILLS_BETA).await
    }

    /// `GET /v1/skills` -- one page of skills.
    pub async fn list(&self, params: ListSkillsParams) -> Result<PaginatedNextPage<Skill>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/skills");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                SKILLS_BETA,
            )
            .await
    }

    /// `GET /v1/skills/{skill_id}` -- fetch a single skill.
    pub async fn get(&self, skill_id: &str) -> Result<Skill> {
        let path = format!("/v1/skills/{skill_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                SKILLS_BETA,
            )
            .await
    }

    /// `DELETE /v1/skills/{skill_id}` -- delete a skill and all its
    /// versions.
    pub async fn delete(&self, skill_id: &str) -> Result<SkillDeleted> {
        let path = format!("/v1/skills/{skill_id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                SKILLS_BETA,
            )
            .await
    }

    /// `POST /v1/skills/{skill_id}/versions` -- upload a new version.
    ///
    /// The version inherits its `display_title` from the parent skill;
    /// the request body carries only the file bundle.
    ///
    /// Multipart bodies are single-use; this method does not retry.
    pub async fn create_version(
        &self,
        skill_id: &str,
        files: Vec<SkillFile>,
    ) -> Result<SkillVersion> {
        let form = build_form(None, &files);
        let path = format!("/v1/skills/{skill_id}/versions");
        let builder = self
            .client
            .request_builder(reqwest::Method::POST, &path)
            .multipart(form);
        self.client.execute(builder, SKILLS_BETA).await
    }

    /// `GET /v1/skills/{skill_id}/versions` -- one page of versions.
    pub async fn list_versions(
        &self,
        skill_id: &str,
        params: ListSkillVersionsParams,
    ) -> Result<PaginatedNextPage<SkillVersion>> {
        let path = format!("/v1/skills/{skill_id}/versions");
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
                SKILLS_BETA,
            )
            .await
    }

    /// `GET /v1/skills/{skill_id}/versions/{version}` -- fetch a single
    /// version.
    pub async fn get_version(&self, skill_id: &str, version: &str) -> Result<SkillVersion> {
        let path = format!("/v1/skills/{skill_id}/versions/{version}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                SKILLS_BETA,
            )
            .await
    }

    /// `DELETE /v1/skills/{skill_id}/versions/{version}` -- delete one
    /// version.
    pub async fn delete_version(
        &self,
        skill_id: &str,
        version: &str,
    ) -> Result<SkillVersionDeleted> {
        let path = format!("/v1/skills/{skill_id}/versions/{version}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                SKILLS_BETA,
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

    fn skill_json(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "type": "skill",
            "display_title": "My Custom Skill",
            "latest_version": "1759178010641129",
            "source": "custom",
            "created_at": "2024-10-30T23:58:27.427722Z",
            "updated_at": "2024-10-30T23:58:27.427722Z"
        })
    }

    fn skill_version_json(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "type": "skill_version",
            "skill_id": "skill_S1",
            "version": "1759178010641129",
            "name": "my-skill",
            "description": "A custom skill",
            "directory": "my-skill",
            "created_at": "2024-10-30T23:58:27.427722Z"
        })
    }

    #[test]
    fn skill_source_round_trips_known_values() {
        let custom: SkillSource = serde_json::from_str(r#""custom""#).unwrap();
        assert_eq!(custom, SkillSource::Custom);
        let anthr: SkillSource = serde_json::from_str(r#""anthropic""#).unwrap();
        assert_eq!(anthr, SkillSource::Anthropic);
        assert_eq!(
            serde_json::to_string(&SkillSource::Custom).unwrap(),
            r#""custom""#
        );
    }

    #[test]
    fn skill_source_falls_through_to_other_for_unknown_values() {
        let s: SkillSource = serde_json::from_str(r#""partner""#).unwrap();
        assert_eq!(s, SkillSource::Other("partner".into()));
        // round-trip
        assert_eq!(serde_json::to_string(&s).unwrap(), r#""partner""#);
    }

    #[tokio::test]
    async fn create_sends_multipart_with_skills_beta_and_decodes_skill() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/skills"))
            .and(header_exists("anthropic-beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(skill_json("skill_C1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateSkillRequest::new()
            .display_title("My Custom Skill")
            .file(SkillFile::new("my-skill/SKILL.md", &b"# my-skill\n"[..]));
        let s = client.skills().create(req).await.unwrap();
        assert_eq!(s.id, "skill_C1");
        assert_eq!(s.source, SkillSource::Custom);

        let recv = &mock.received_requests().await.unwrap()[0];
        let beta = recv
            .headers
            .get("anthropic-beta")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(beta.contains("skills-2025-10-02"), "{beta}");
        let ct = recv.headers.get("content-type").unwrap().to_str().unwrap();
        assert!(ct.starts_with("multipart/form-data"), "{ct}");
    }

    #[tokio::test]
    async fn list_passes_limit_page_source_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/skills"))
            .and(query_param("limit", "5"))
            .and(query_param("page", "page_abc"))
            .and(query_param("source", "anthropic"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [skill_json("skill_L1")],
                "has_more": true,
                "next_page": "page_def"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .skills()
            .list(
                ListSkillsParams::default()
                    .limit(5)
                    .page("page_abc")
                    .source(SkillSourceFilter::Anthropic),
            )
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
        assert!(page.has_more);
        assert_eq!(page.next_cursor(), Some("page_def"));
    }

    #[tokio::test]
    async fn get_decodes_single_skill() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/skills/skill_G1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(skill_json("skill_G1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let s = client.skills().get("skill_G1").await.unwrap();
        assert_eq!(s.id, "skill_G1");
        assert_eq!(s.kind, "skill");
        assert_eq!(s.latest_version, "1759178010641129");
    }

    #[tokio::test]
    async fn delete_returns_skill_deleted_envelope() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/skills/skill_D1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "skill_D1",
                "type": "skill_deleted"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let confirm = client.skills().delete("skill_D1").await.unwrap();
        assert_eq!(confirm.id, "skill_D1");
        assert_eq!(confirm.kind, "skill_deleted");
    }

    #[tokio::test]
    async fn create_version_sends_files_only_multipart() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/skills/skill_S1/versions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(skill_version_json("skillver_V1")),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let v = client
            .skills()
            .create_version(
                "skill_S1",
                vec![SkillFile::new(
                    "my-skill/SKILL.md",
                    &b"# updated skill\n"[..],
                )],
            )
            .await
            .unwrap();
        assert_eq!(v.id, "skillver_V1");
        assert_eq!(v.skill_id, "skill_S1");
        assert_eq!(v.version, "1759178010641129");
        assert_eq!(v.directory, "my-skill");

        let recv = &mock.received_requests().await.unwrap()[0];
        let body = String::from_utf8_lossy(&recv.body);
        assert!(
            !body.contains("name=\"display_title\""),
            "create_version must not include display_title"
        );
        assert!(body.contains("name=\"files\""), "{body}");
    }

    #[tokio::test]
    async fn list_versions_passes_skill_id_and_query() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/skills/skill_S1/versions"))
            .and(query_param("limit", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [skill_version_json("skillver_LV1")],
                "has_more": false,
                "next_page": null
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .skills()
            .list_versions("skill_S1", ListSkillVersionsParams::default().limit(50))
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
        assert!(!page.has_more);
        assert_eq!(page.next_cursor(), None);
    }

    #[tokio::test]
    async fn get_version_decodes_single_version() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/skills/skill_S1/versions/1759178010641129"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(skill_version_json("skillver_GV1")),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let v = client
            .skills()
            .get_version("skill_S1", "1759178010641129")
            .await
            .unwrap();
        assert_eq!(v.id, "skillver_GV1");
        assert_eq!(v.kind, "skill_version");
    }

    #[tokio::test]
    async fn delete_version_id_is_version_timestamp_not_skillver() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/skills/skill_S1/versions/1759178010641129"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "1759178010641129",
                "type": "skill_version_deleted"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let confirm = client
            .skills()
            .delete_version("skill_S1", "1759178010641129")
            .await
            .unwrap();
        // Note: the delete-version endpoint returns the *version* string in
        // `id`, not the `skillver_*` identifier. This is intentional per
        // the API spec.
        assert_eq!(confirm.id, "1759178010641129");
        assert_eq!(confirm.kind, "skill_version_deleted");
    }

    #[tokio::test]
    async fn skill_file_from_path_reads_disk_and_uses_filename_as_path() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("claude_api_skill_{}_SKILL.md", std::process::id()));
        tokio::fs::write(&p, b"# from disk\n").await.unwrap();
        let f = SkillFile::from_path(&p).await.unwrap();
        assert!(f.path.ends_with("_SKILL.md"));
        assert_eq!(&f.contents[..], b"# from disk\n");
        tokio::fs::remove_file(&p).await.ok();
    }
}
