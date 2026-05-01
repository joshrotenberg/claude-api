//! The async `Batches<'a>` namespace.

#![cfg(feature = "async")]

use std::time::Instant;

use futures_util::stream::{BoxStream, Stream, StreamExt};
use serde::Serialize;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::pagination::Paginated;

use super::types::{
    BatchDeleted, BatchRequest, BatchResultItem, ListBatchesParams, MessageBatch, ProcessingStatus,
    WaitOptions,
};

/// Namespace handle for the Batches API.
pub struct Batches<'a> {
    client: &'a Client,
}

impl<'a> Batches<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Submit a batch of [`BatchRequest`] entries. Returns the initial
    /// [`MessageBatch`] status (typically `processing_status: in_progress`).
    pub async fn create(&self, requests: Vec<BatchRequest>) -> Result<MessageBatch> {
        #[derive(Serialize)]
        struct Envelope<'r> {
            requests: &'r [BatchRequest],
        }
        let envelope = Envelope {
            requests: &requests,
        };
        let envelope_ref = &envelope;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/messages/batches")
                        .json(envelope_ref)
                },
                &[],
            )
            .await
    }

    /// Fetch the current status of a batch by id.
    pub async fn get(&self, id: &str) -> Result<MessageBatch> {
        let path = format!("/v1/messages/batches/{id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[],
            )
            .await
    }

    /// Fetch one page of batches.
    pub async fn list(&self, params: ListBatchesParams) -> Result<Paginated<MessageBatch>> {
        let params_ref = &params;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::GET, "/v1/messages/batches")
                        .query(params_ref)
                },
                &[],
            )
            .await
    }

    /// Fetch every batch, transparently paging.
    pub async fn list_all(&self) -> Result<Vec<MessageBatch>> {
        let mut all = Vec::new();
        let mut params = ListBatchesParams::default();
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

    /// Request cancellation of a batch. Already-running entries continue
    /// until they finish; the batch transitions to `Canceling` and then
    /// to `Ended` once those settle.
    pub async fn cancel(&self, id: &str) -> Result<MessageBatch> {
        let path = format!("/v1/messages/batches/{id}/cancel");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[],
            )
            .await
    }

    /// Delete a batch. Allowed only after the batch has ended.
    pub async fn delete(&self, id: &str) -> Result<BatchDeleted> {
        let path = format!("/v1/messages/batches/{id}");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::DELETE, &path),
                &[],
            )
            .await
    }

    /// Poll [`Self::get`] until the batch's `processing_status` is
    /// [`ProcessingStatus::Ended`] (or any other terminal status the
    /// server reports). Returns the final [`MessageBatch`].
    ///
    /// Honors [`WaitOptions::poll_interval`] between calls and
    /// [`WaitOptions::timeout`] as an overall ceiling.
    pub async fn wait_for(&self, id: &str, options: WaitOptions) -> Result<MessageBatch> {
        let started = Instant::now();
        loop {
            let batch = self.get(id).await?;
            if matches!(
                batch.processing_status,
                ProcessingStatus::Ended | ProcessingStatus::Other
            ) {
                return Ok(batch);
            }
            if let Some(timeout) = options.timeout
                && started.elapsed() >= timeout
            {
                return Err(Error::InvalidConfig(format!(
                    "wait_for({id}) timed out after {:?}",
                    started.elapsed()
                )));
            }
            tokio::time::sleep(options.poll_interval).await;
        }
    }

    /// Fetch all batch results into a Vec. Convenience wrapper over
    /// [`Self::results_stream`] for callers that don't need streaming.
    pub async fn results(&self, id: &str) -> Result<Vec<BatchResultItem>> {
        let mut stream = self.results_stream(id).await?;
        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            out.push(item?);
        }
        Ok(out)
    }

    /// Stream the JSONL results body line-by-line, decoding each line as
    /// a [`BatchResultItem`]. Returns immediately after the connection
    /// is established; consumes lazily as the caller polls the stream.
    ///
    /// Mid-stream connection failures are surfaced as stream items;
    /// retries are *not* applied (consistent with the SSE streaming
    /// design -- silent retry would drop content).
    pub async fn results_stream(
        &self,
        id: &str,
    ) -> Result<BoxStream<'static, Result<BatchResultItem>>> {
        let path = format!("/v1/messages/batches/{id}/results");
        let response = self
            .client
            .execute_streaming(
                self.client.request_builder(reqwest::Method::GET, &path),
                &[],
            )
            .await?;
        Ok(jsonl_stream(response).boxed())
    }
}

/// Wrap a streaming response body in a `Stream` that yields one decoded
/// `T` per JSONL line.
fn jsonl_stream<T>(response: reqwest::Response) -> impl Stream<Item = Result<T>> + Send + 'static
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    futures_util::stream::unfold(
        (response.bytes_stream(), Vec::<u8>::new(), false),
        |(mut bytes, mut buffer, done)| async move {
            if done && buffer.is_empty() {
                return None;
            }
            loop {
                // Try to extract the next complete line from the buffer.
                if let Some(newline_idx) = buffer.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=newline_idx).collect();
                    let trimmed = trim_trailing_newline(&line);
                    if trimmed.is_empty() {
                        // Skip blank lines; loop to extract the next.
                        continue;
                    }
                    let parsed: Result<T> = serde_json::from_slice(trimmed).map_err(Error::from);
                    return Some((parsed, (bytes, buffer, done)));
                }

                // Need more bytes from the upstream stream.
                match bytes.next().await {
                    Some(Ok(chunk)) => buffer.extend_from_slice(&chunk),
                    Some(Err(e)) => {
                        return Some((Err(Error::from(e)), (bytes, buffer, true)));
                    }
                    None => {
                        // Upstream EOF. Flush any trailing partial line.
                        if buffer.is_empty() {
                            return None;
                        }
                        let trimmed = trim_trailing_newline(&buffer);
                        let parsed: Result<T> =
                            serde_json::from_slice(trimmed).map_err(Error::from);
                        buffer.clear();
                        return Some((parsed, (bytes, buffer, true)));
                    }
                }
            }
        },
    )
}

fn trim_trailing_newline(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batches::types::BatchResultPayload;
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

    fn batch_in_progress() -> serde_json::Value {
        json!({
            "id": "msgbatch_01",
            "type": "message_batch",
            "processing_status": "in_progress",
            "request_counts": {
                "processing": 2, "succeeded": 0, "errored": 0,
                "canceled": 0, "expired": 0
            },
            "created_at": "2026-04-30T00:00:00Z",
            "expires_at": "2026-05-01T00:00:00Z"
        })
    }

    fn batch_ended() -> serde_json::Value {
        json!({
            "id": "msgbatch_01",
            "type": "message_batch",
            "processing_status": "ended",
            "request_counts": {
                "processing": 0, "succeeded": 2, "errored": 0,
                "canceled": 0, "expired": 0
            },
            "created_at": "2026-04-30T00:00:00Z",
            "expires_at": "2026-05-01T00:00:00Z",
            "ended_at": "2026-04-30T01:00:00Z",
            "results_url": "https://example/results"
        })
    }

    #[tokio::test]
    async fn create_posts_envelope_with_requests_array() {
        use crate::messages::request::CreateMessageRequest;
        use crate::types::ModelId;

        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages/batches"))
            .and(body_partial_json(json!({
                "requests": [
                    {
                        "custom_id": "r1",
                        "params": {
                            "model": "claude-sonnet-4-6",
                            "max_tokens": 8,
                            "messages": [{"role": "user", "content": "hi"}]
                        }
                    }
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(batch_in_progress()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("hi")
            .build()
            .unwrap();
        let batch = client
            .batches()
            .create(vec![BatchRequest::new("r1", req)])
            .await
            .unwrap();
        assert_eq!(batch.id, "msgbatch_01");
        assert_eq!(batch.processing_status, ProcessingStatus::InProgress);
    }

    #[tokio::test]
    async fn get_returns_status_for_id() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(batch_ended()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let b = client.batches().get("msgbatch_01").await.unwrap();
        assert_eq!(b.processing_status, ProcessingStatus::Ended);
        assert_eq!(b.request_counts.succeeded, 2);
    }

    #[tokio::test]
    async fn cancel_transitions_to_canceling() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages/batches/msgbatch_01/cancel"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msgbatch_01",
                "type": "message_batch",
                "processing_status": "canceling",
                "request_counts": {
                    "processing": 1, "succeeded": 0, "errored": 0,
                    "canceled": 1, "expired": 0
                },
                "created_at": "2026-04-30T00:00:00Z",
                "expires_at": "2026-05-01T00:00:00Z",
                "cancel_initiated_at": "2026-04-30T00:30:00Z"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let b = client.batches().cancel("msgbatch_01").await.unwrap();
        assert_eq!(b.processing_status, ProcessingStatus::Canceling);
        assert!(b.cancel_initiated_at.is_some());
    }

    #[tokio::test]
    async fn delete_returns_typed_confirmation() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/messages/batches/msgbatch_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msgbatch_01",
                "type": "message_batch_deleted"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let d = client.batches().delete("msgbatch_01").await.unwrap();
        assert_eq!(d.id, "msgbatch_01");
        assert_eq!(d.kind, "message_batch_deleted");
    }

    #[tokio::test]
    async fn list_returns_paginated_envelope() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [batch_in_progress()],
                "has_more": false,
                "first_id": "msgbatch_01",
                "last_id": "msgbatch_01"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .batches()
            .list(ListBatchesParams::default())
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn wait_for_polls_until_ended() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(batch_in_progress()))
            .up_to_n_times(2)
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(batch_ended()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let opts = WaitOptions::default()
            .poll_interval(std::time::Duration::from_millis(1))
            .timeout(std::time::Duration::from_secs(5));
        let final_batch = client
            .batches()
            .wait_for("msgbatch_01", opts)
            .await
            .unwrap();
        assert_eq!(final_batch.processing_status, ProcessingStatus::Ended);
    }

    #[tokio::test]
    async fn wait_for_honors_timeout() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(batch_in_progress()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let opts = WaitOptions::default()
            .poll_interval(std::time::Duration::from_millis(1))
            .timeout(std::time::Duration::from_millis(20));
        let err = client
            .batches()
            .wait_for("msgbatch_01", opts)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
    }

    #[tokio::test]
    async fn results_decodes_jsonl_into_typed_items() {
        let jsonl = "\
{\"custom_id\":\"r1\",\"result\":{\"type\":\"succeeded\",\"message\":{\"id\":\"m1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"a\"}],\"model\":\"claude-sonnet-4-6\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}}
{\"custom_id\":\"r2\",\"result\":{\"type\":\"errored\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"slow\"}}}
{\"custom_id\":\"r3\",\"result\":{\"type\":\"canceled\"}}
";
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01/results"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/x-jsonl")
                    .set_body_string(jsonl),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let items = client.batches().results("msgbatch_01").await.unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].custom_id, "r1");
        assert!(matches!(
            items[0].result,
            BatchResultPayload::Succeeded { .. }
        ));
        assert_eq!(items[1].custom_id, "r2");
        assert!(matches!(
            items[1].result,
            BatchResultPayload::Errored { .. }
        ));
        assert!(matches!(items[2].result, BatchResultPayload::Canceled));
    }

    #[tokio::test]
    async fn results_stream_yields_items_lazily() {
        let jsonl = "\
{\"custom_id\":\"a\",\"result\":{\"type\":\"canceled\"}}
{\"custom_id\":\"b\",\"result\":{\"type\":\"expired\"}}
";
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01/results"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/x-jsonl")
                    .set_body_string(jsonl),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut stream = client
            .batches()
            .results_stream("msgbatch_01")
            .await
            .unwrap();

        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.custom_id, "a");
        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(second.custom_id, "b");
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn results_stream_skips_blank_lines() {
        let jsonl = concat!(
            "\n",
            "{\"custom_id\":\"a\",\"result\":{\"type\":\"canceled\"}}\n",
            "\n",
            "{\"custom_id\":\"b\",\"result\":{\"type\":\"expired\"}}\n",
            "\n",
        );
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01/results"))
            .respond_with(ResponseTemplate::new(200).set_body_string(jsonl))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let items = client.batches().results("msgbatch_01").await.unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn results_stream_handles_missing_trailing_newline() {
        // Last line has no trailing \n; must still be emitted.
        let jsonl = "{\"custom_id\":\"a\",\"result\":{\"type\":\"canceled\"}}\n{\"custom_id\":\"b\",\"result\":{\"type\":\"expired\"}}";
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/messages/batches/msgbatch_01/results"))
            .respond_with(ResponseTemplate::new(200).set_body_string(jsonl))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let items = client.batches().results("msgbatch_01").await.unwrap();
        assert_eq!(items.len(), 2);
    }
}
