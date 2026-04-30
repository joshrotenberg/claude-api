//! Wire types for the Files API.

use serde::{Deserialize, Serialize};

/// Metadata for a single uploaded file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileMetadata {
    /// Stable file identifier (e.g. `file_011...`).
    pub id: String,
    /// Wire `type` discriminant; always `"file"`.
    #[serde(rename = "type", default = "default_file_kind")]
    pub kind: String,
    /// Original filename supplied at upload.
    pub filename: String,
    /// IANA MIME type (e.g. `application/pdf`).
    pub mime_type: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Creation timestamp (ISO-8601).
    pub created_at: String,
    /// Whether the file's bytes can be retrieved via
    /// [`Files::download`](super::api::Files::download).
    #[serde(default)]
    pub downloadable: bool,
}

fn default_file_kind() -> String {
    "file".to_owned()
}

/// Confirmation returned by `DELETE /v1/files/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileDeleted {
    /// ID of the deleted file.
    pub id: String,
    /// Wire `type`; typically `"file_deleted"`.
    #[serde(rename = "type", default)]
    pub kind: String,
}

/// Query parameters for `GET /v1/files`.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct ListFilesParams {
    /// Cursor for backward pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    /// Cursor for forward pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
    /// Page size (server-defaulted if omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl ListFilesParams {
    /// Set the `after_id` cursor.
    #[must_use]
    pub fn after_id(mut self, id: impl Into<String>) -> Self {
        self.after_id = Some(id.into());
        self
    }

    /// Set the `before_id` cursor.
    #[must_use]
    pub fn before_id(mut self, id: impl Into<String>) -> Self {
        self.before_id = Some(id.into());
        self
    }

    /// Set the page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn file_metadata_round_trips() {
        let raw = json!({
            "id": "file_011ABC",
            "type": "file",
            "filename": "report.pdf",
            "mime_type": "application/pdf",
            "size_bytes": 12345,
            "created_at": "2026-04-30T00:00:00Z",
            "downloadable": true
        });
        let parsed: FileMetadata = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(parsed.id, "file_011ABC");
        assert_eq!(parsed.kind, "file");
        assert_eq!(parsed.filename, "report.pdf");
        assert_eq!(parsed.size_bytes, 12345);
        assert!(parsed.downloadable);
        assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
    }

    #[test]
    fn file_metadata_kind_defaults_when_missing() {
        let raw = json!({
            "id": "file_X",
            "filename": "x.txt",
            "mime_type": "text/plain",
            "size_bytes": 1,
            "created_at": "2026"
        });
        let parsed: FileMetadata = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.kind, "file");
    }

    #[test]
    fn file_deleted_round_trips() {
        let raw = json!({"id": "file_X", "type": "file_deleted"});
        let parsed: FileDeleted = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.id, "file_X");
        assert_eq!(parsed.kind, "file_deleted");
    }
}
