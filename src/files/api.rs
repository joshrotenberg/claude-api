//! The async `Files<'a>` namespace.

#![cfg(feature = "async")]

use std::path::Path;

use bytes::Bytes;
use futures_util::stream::TryStreamExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::io::{ReaderStream, StreamReader};

use crate::client::Client;
use crate::error::{Error, Result};
use crate::pagination::Paginated;

use super::types::{FileDeleted, FileMetadata, ListFilesParams};

/// Beta version tag attached to every Files-API request.
const FILES_BETA: &[&str] = &["files-api-2025-04-14"];

/// Namespace handle for the Files API.
pub struct Files<'a> {
    client: &'a Client,
}

impl<'a> Files<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Upload a file from disk.
    ///
    /// Streams the file body through the request without buffering it in
    /// memory; suitable for large PDFs.
    ///
    /// `media_type` defaults to `application/octet-stream`. The filename is
    /// taken from the path's file name component.
    pub async fn upload_path(&self, path: impl AsRef<Path>) -> Result<FileMetadata> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                Error::InvalidConfig(format!("invalid filename in path {}", path.display()))
            })?
            .to_owned();
        let media_type = guess_media_type(&filename).unwrap_or("application/octet-stream");
        let file = tokio::fs::File::open(path).await?;
        self.upload_stream(file, filename, media_type).await
    }

    /// Upload from any [`AsyncRead`] source. The body is streamed; not
    /// buffered. Retries are *not* applied to uploads -- the source is
    /// consumed.
    pub async fn upload_stream<R>(
        &self,
        reader: R,
        filename: impl Into<String>,
        media_type: impl Into<String>,
    ) -> Result<FileMetadata>
    where
        R: AsyncRead + Send + Sync + 'static,
    {
        let filename = filename.into();
        let media_type = media_type.into();
        let stream = ReaderStream::new(Box::pin(reader));
        let body = reqwest::Body::wrap_stream(stream);
        let part = reqwest::multipart::Part::stream(body)
            .file_name(filename)
            .mime_str(&media_type)
            .map_err(|e| Error::InvalidConfig(format!("invalid media_type for upload: {e}")))?;
        self.upload_with_part(part).await
    }

    /// Upload from a `Bytes` buffer (or anything that converts to `Bytes`).
    /// Suitable for small payloads where streaming is overkill.
    pub async fn upload_bytes(
        &self,
        bytes: impl Into<Bytes>,
        filename: impl Into<String>,
        media_type: impl Into<String>,
    ) -> Result<FileMetadata> {
        let filename = filename.into();
        let media_type = media_type.into();
        let part = reqwest::multipart::Part::bytes(bytes.into().to_vec())
            .file_name(filename)
            .mime_str(&media_type)
            .map_err(|e| Error::InvalidConfig(format!("invalid media_type for upload: {e}")))?;
        self.upload_with_part(part).await
    }

    async fn upload_with_part(&self, part: reqwest::multipart::Part) -> Result<FileMetadata> {
        let form = reqwest::multipart::Form::new().part("file", part);
        // No retry: multipart bodies built from streaming sources are
        // single-use. Users who need retry should wrap their own loop
        // around upload_path / upload_bytes.
        let builder = self
            .client
            .request_builder(reqwest::Method::POST, "/v1/files")
            .multipart(form);
        self.client.execute(builder, FILES_BETA).await
    }

    /// Fetch metadata for a single file by ID.
    pub async fn get(&self, id: &str) -> Result<FileMetadata> {
        let path = format!("/v1/files/{id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                FILES_BETA,
            )
            .await
    }

    /// Fetch one page of file metadata.
    pub async fn list(&self, params: ListFilesParams) -> Result<Paginated<FileMetadata>> {
        let params_ref = &params;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::GET, "/v1/files")
                        .query(params_ref)
                },
                FILES_BETA,
            )
            .await
    }

    /// Fetch every file's metadata, transparently paging.
    pub async fn list_all(&self) -> Result<Vec<FileMetadata>> {
        let mut all = Vec::new();
        let mut params = ListFilesParams::default();
        loop {
            let page = self.list(params.clone()).await?;
            let next_cursor = page.next_after().map(str::to_owned);
            all.extend(page.data);
            match next_cursor {
                Some(cursor) => params.after_id = Some(cursor),
                None => break,
            }
        }
        Ok(all)
    }

    /// Delete a file by ID. Returns the deletion confirmation.
    pub async fn delete(&self, id: &str) -> Result<FileDeleted> {
        let path = format!("/v1/files/{id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                FILES_BETA,
            )
            .await
    }

    /// Download a file's bytes into memory. Suitable for small files; for
    /// streaming to disk or a network sink, use [`Self::download_to`].
    pub async fn download(&self, id: &str) -> Result<Bytes> {
        let path = format!("/v1/files/{id}/content");
        let response = self
            .client
            .execute_streaming(
                self.client.request_builder(reqwest::Method::GET, &path),
                FILES_BETA,
            )
            .await?;
        Ok(response.bytes().await?)
    }

    /// Stream a file's bytes into any [`AsyncWrite`] sink. Returns the
    /// total number of bytes written.
    pub async fn download_to<W>(&self, id: &str, writer: &mut W) -> Result<u64>
    where
        W: AsyncWrite + Unpin,
    {
        let path = format!("/v1/files/{id}/content");
        let response = self
            .client
            .execute_streaming(
                self.client.request_builder(reqwest::Method::GET, &path),
                FILES_BETA,
            )
            .await?;
        let stream = response
            .bytes_stream()
            .map_err(|e| std::io::Error::other(e.to_string()));
        let mut reader = StreamReader::new(stream);
        let copied = tokio::io::copy(&mut reader, writer).await?;
        Ok(copied)
    }
}

/// Best-effort MIME type from a filename extension. Returns `None` for
/// extensions not in the small built-in table; callers can still pass any
/// `media_type` explicitly via [`Files::upload_stream`] / [`Files::upload_bytes`].
fn guess_media_type(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn file_metadata_json(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "type": "file",
            "filename": "test.pdf",
            "mime_type": "application/pdf",
            "size_bytes": 4,
            "created_at": "2026-04-30T00:00:00Z",
            "downloadable": true
        })
    }

    #[tokio::test]
    async fn upload_bytes_sends_multipart_and_decodes_metadata() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/files"))
            .and(header_exists("anthropic-beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_metadata_json("file_b1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let meta = client
            .files()
            .upload_bytes(Bytes::from_static(b"abcd"), "test.pdf", "application/pdf")
            .await
            .unwrap();
        assert_eq!(meta.id, "file_b1");
        assert_eq!(meta.size_bytes, 4);

        // Verify the beta header is the Files-API tag.
        let req = &mock.received_requests().await.unwrap()[0];
        let beta = req.headers.get("anthropic-beta").unwrap().to_str().unwrap();
        assert!(beta.contains("files-api-"), "{beta}");
    }

    #[tokio::test]
    async fn upload_path_streams_real_file_from_disk() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_metadata_json("file_p1")))
            .mount(&mock)
            .await;

        let dir = std::env::temp_dir();
        let path = dir.join(format!("claude_api_test_{}.txt", std::process::id()));
        std::fs::write(&path, b"hello from disk").unwrap();

        let client = client_for(&mock);
        let meta = client.files().upload_path(&path).await.unwrap();
        assert_eq!(meta.id, "file_p1");

        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn upload_stream_accepts_any_async_read() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_metadata_json("file_s1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        // &[u8] satisfies AsyncRead through tokio's blanket impl, but we need
        // owned + 'static. Wrap in a Cursor over Vec<u8>.
        let reader = std::io::Cursor::new(b"streamed bytes".to_vec());
        let meta = client
            .files()
            .upload_stream(reader, "stream.txt", "text/plain")
            .await
            .unwrap();
        assert_eq!(meta.id, "file_s1");
    }

    #[tokio::test]
    async fn get_returns_metadata_for_id() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/files/file_g1"))
            .and(header_exists("anthropic-beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_metadata_json("file_g1")))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let meta = client.files().get("file_g1").await.unwrap();
        assert_eq!(meta.id, "file_g1");
    }

    #[tokio::test]
    async fn list_returns_paginated_envelope() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [file_metadata_json("file_l1"), file_metadata_json("file_l2")],
                "has_more": false,
                "first_id": "file_l1",
                "last_id": "file_l2"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .files()
            .list(ListFilesParams::default())
            .await
            .unwrap();
        assert_eq!(page.data.len(), 2);
    }

    #[tokio::test]
    async fn delete_returns_typed_confirmation() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/files/file_d1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "file_d1",
                "type": "file_deleted"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let confirm = client.files().delete("file_d1").await.unwrap();
        assert_eq!(confirm.id, "file_d1");
        assert_eq!(confirm.kind, "file_deleted");
    }

    #[tokio::test]
    async fn download_returns_file_bytes() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/files/file_dl1/content"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"file payload bytes".to_vec()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let bytes = client.files().download("file_dl1").await.unwrap();
        assert_eq!(&bytes[..], b"file payload bytes");
    }

    #[tokio::test]
    async fn download_to_streams_into_async_write() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/files/file_dl2/content"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"streamed download".to_vec()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut sink: Vec<u8> = Vec::new();
        let bytes_written = client
            .files()
            .download_to("file_dl2", &mut sink)
            .await
            .unwrap();
        assert_eq!(bytes_written, b"streamed download".len() as u64);
        assert_eq!(&sink[..], b"streamed download");
    }

    #[tokio::test]
    async fn download_propagates_404_with_request_id() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/files/missing/content"))
            .respond_with(
                ResponseTemplate::new(404)
                    .insert_header("request-id", "req_404")
                    .set_body_json(json!({
                        "type": "error",
                        "error": {"type": "not_found_error", "message": "no such file"}
                    })),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let err = client.files().download("missing").await.unwrap_err();
        assert_eq!(err.status(), Some(http::StatusCode::NOT_FOUND));
        assert_eq!(err.request_id(), Some("req_404"));
    }

    #[test]
    fn guess_media_type_handles_common_extensions() {
        assert_eq!(guess_media_type("doc.pdf"), Some("application/pdf"));
        assert_eq!(guess_media_type("notes.MD"), Some("text/plain"));
        assert_eq!(guess_media_type("photo.jpg"), Some("image/jpeg"));
        assert_eq!(guess_media_type("photo.JPEG"), Some("image/jpeg"));
        assert_eq!(guess_media_type("data.unknown"), None);
        assert_eq!(guess_media_type("noext"), None);
    }
}
