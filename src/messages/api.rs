//! The Messages namespace: [`Messages::create`], [`Messages::count_tokens`],
//! and their `_with_beta` siblings.
//!
//! Obtain via [`Client::messages`](crate::Client::messages).
//!
//! Streaming (`create_stream`) lands in task #12.

#![cfg(feature = "async")]

use crate::client::Client;
use crate::error::Result;
use crate::messages::request::{CountTokensRequest, CreateMessageRequest};
use crate::messages::response::{CountTokensResponse, Message};

#[cfg(feature = "streaming")]
use crate::messages::stream::EventStream;

/// Namespace handle for the Messages API.
///
/// Obtained via [`Client::messages`](crate::Client::messages); cheap to
/// construct (just borrows the client).
pub struct Messages<'a> {
    client: &'a Client,
}

impl<'a> Messages<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Send a request to `POST /v1/messages` and return the full response.
    ///
    /// Retries are governed by the [`RetryPolicy`](crate::retry::RetryPolicy)
    /// configured on the client.
    pub async fn create(&self, request: CreateMessageRequest) -> Result<Message> {
        self.create_with_beta(request, &[]).await
    }

    /// Like [`Self::create`] but with additional per-request beta headers
    /// merged into `anthropic-beta`.
    pub async fn create_with_beta(
        &self,
        request: CreateMessageRequest,
        betas: &[&str],
    ) -> Result<Message> {
        let request_ref = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/messages")
                        .json(request_ref)
                },
                betas,
            )
            .await
    }

    /// Count the input tokens that would be billed for the given request,
    /// without invoking the model.
    pub async fn count_tokens(&self, request: CountTokensRequest) -> Result<CountTokensResponse> {
        self.count_tokens_with_beta(request, &[]).await
    }

    /// Like [`Self::count_tokens`] but with per-request beta headers.
    pub async fn count_tokens_with_beta(
        &self,
        request: CountTokensRequest,
        betas: &[&str],
    ) -> Result<CountTokensResponse> {
        let request_ref = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/messages/count_tokens")
                        .json(request_ref)
                },
                betas,
            )
            .await
    }

    /// Open a streaming connection to `POST /v1/messages` and return an
    /// [`EventStream`].
    ///
    /// The returned stream yields [`StreamEvent`](crate::messages::stream::StreamEvent)s
    /// as they arrive; call [`EventStream::aggregate`] to drive it to
    /// completion and reconstruct the final [`Message`].
    ///
    /// Streaming requests are *not* retried.
    #[cfg(feature = "streaming")]
    #[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
    pub async fn create_stream(&self, request: CreateMessageRequest) -> Result<EventStream> {
        self.create_stream_with_beta(request, &[]).await
    }

    /// Like [`Self::create_stream`] but with per-request beta headers.
    #[cfg(feature = "streaming")]
    #[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
    pub async fn create_stream_with_beta(
        &self,
        mut request: CreateMessageRequest,
        betas: &[&str],
    ) -> Result<EventStream> {
        request.stream = true;
        let response = self
            .client
            .execute_streaming(
                self.client
                    .request_builder(reqwest::Method::POST, "/v1/messages")
                    .json(&request),
                betas,
            )
            .await?;
        Ok(EventStream::from_response(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::input::MessageInput;
    use crate::messages::response::Message;
    use crate::types::{ModelId, Role, StopReason};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    fn fake_response_body() -> serde_json::Value {
        json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hi!"}],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 5, "output_tokens": 2}
        })
    }

    #[tokio::test]
    async fn create_posts_to_v1_messages_with_typed_request_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "sk-ant-test"))
            .and(header("anthropic-version", crate::ANTHROPIC_VERSION))
            .and(body_partial_json(json!({
                "model": "claude-sonnet-4-6",
                "max_tokens": 64,
                "messages": [{"role": "user", "content": "hi"}]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response_body()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(64)
            .user("hi")
            .build()
            .unwrap();
        let resp = client.messages().create(req).await.unwrap();

        assert_eq!(resp.id, "msg_test");
        assert_eq!(resp.role, Role::Assistant);
        assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(resp.usage.input_tokens, 5);
    }

    #[tokio::test]
    async fn create_with_beta_attaches_per_request_beta_header() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header_exists("anthropic-beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response_body()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("x")
            .build()
            .unwrap();

        let _: Message = client
            .messages()
            .create_with_beta(req, &["computer-use-2025-01-24"])
            .await
            .unwrap();

        let received = &mock.received_requests().await.unwrap()[0];
        let beta = received
            .headers
            .get("anthropic-beta")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(beta, "computer-use-2025-01-24");
    }

    #[tokio::test]
    async fn create_propagates_api_error_with_request_id() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(400)
                    .insert_header("request-id", "req_xyz")
                    .set_body_json(json!({
                        "type": "error",
                        "error": {"type": "invalid_request_error", "message": "bad input"}
                    })),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("x")
            .build()
            .unwrap();

        let err = client.messages().create(req).await.unwrap_err();
        assert_eq!(err.request_id(), Some("req_xyz"));
        assert_eq!(err.status(), Some(http::StatusCode::BAD_REQUEST));
    }

    #[tokio::test]
    async fn count_tokens_posts_to_count_tokens_endpoint() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages/count_tokens"))
            .and(body_partial_json(json!({
                "model": "claude-haiku-4-5-20251001",
                "messages": [{"role": "user", "content": "x"}]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"input_tokens": 7})))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CountTokensRequest::builder()
            .model(ModelId::HAIKU_4_5)
            .user("x")
            .build()
            .unwrap();
        let resp = client.messages().count_tokens(req).await.unwrap();
        assert_eq!(resp.input_tokens, 7);
    }

    #[tokio::test]
    async fn create_appends_assistant_prefill_in_history() {
        // Verifies that the .assistant() builder method works end-to-end.
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "content": "Sure, "}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response_body()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("hi")
            .assistant("Sure, ")
            .build()
            .unwrap();
        let _ = client.messages().create(req).await.unwrap();
    }

    #[tokio::test]
    async fn create_retries_on_overloaded_then_succeeds() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(529))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_response_body()))
            .mount(&mock)
            .await;

        // Use a tiny retry policy so the test is fast.
        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .retry(crate::retry::RetryPolicy {
                max_attempts: 3,
                initial_backoff: std::time::Duration::from_millis(1),
                max_backoff: std::time::Duration::from_millis(5),
                jitter: crate::retry::Jitter::None,
                respect_retry_after: false,
            })
            .build()
            .unwrap();

        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("x")
            .build()
            .unwrap();
        let resp = client.messages().create(req).await.unwrap();
        assert_eq!(resp.id, "msg_test");
        assert_eq!(mock.received_requests().await.unwrap().len(), 2);
    }

    #[test]
    fn messages_namespace_borrows_client() {
        // Sanity check the borrow shape: dropping the messages handle leaves
        // the client usable.
        let client = Client::new("sk-ant-x");
        {
            let _m = client.messages();
        }
        let _ = client.messages();

        // Suppress unused warning for MessageInput (it's used transitively via builder).
        let _: MessageInput = MessageInput::user("x");
    }

    // -----------------------------------------------------------------
    // Streaming end-to-end (gated on streaming feature)
    // -----------------------------------------------------------------

    #[cfg(feature = "streaming")]
    fn sse_corpus() -> &'static str {
        // A typical Hello-world streamed message, end to end.
        concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_S\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-6\",\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n",
            "\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n",
            "\n",
            "event: ping\n",
            "data: {\"type\":\"ping\"}\n",
            "\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n",
            "\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n",
            "\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
            "\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}\n",
            "\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n",
            "\n",
        )
    }

    #[cfg(feature = "streaming")]
    #[tokio::test]
    async fn create_stream_aggregates_to_full_message() {
        use crate::messages::content::{ContentBlock, KnownBlock};
        use crate::messages::stream::EventStream;

        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({"stream": true})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_corpus()),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("hi")
            .build()
            .unwrap();

        let stream: EventStream = client.messages().create_stream(req).await.unwrap();
        let msg = stream.aggregate().await.unwrap();

        assert_eq!(msg.id, "msg_S");
        assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(msg.usage.output_tokens, 2);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Known(KnownBlock::Text { text, .. }) => {
                assert_eq!(text, "Hello world");
            }
            _ => panic!("expected text block"),
        }
    }

    #[cfg(feature = "streaming")]
    #[tokio::test]
    async fn create_stream_yields_individual_events_for_iterator_use() {
        use futures_util::StreamExt;

        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_corpus()),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("hi")
            .build()
            .unwrap();

        let mut stream = client.messages().create_stream(req).await.unwrap();
        let mut count = 0;
        let mut saw_message_stop = false;
        while let Some(ev) = stream.next().await {
            let ev = ev.unwrap();
            count += 1;
            if ev.type_tag() == Some("message_stop") {
                saw_message_stop = true;
            }
        }
        assert!(saw_message_stop, "expected to see message_stop event");
        assert!(count >= 7, "expected at least 7 events, got {count}");
    }

    #[cfg(feature = "streaming")]
    #[tokio::test]
    async fn create_stream_propagates_connect_error() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(401)
                    .insert_header("request-id", "req_unauth")
                    .set_body_json(json!({
                        "type": "error",
                        "error": {"type": "authentication_error", "message": "bad key"}
                    })),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("hi")
            .build()
            .unwrap();

        let err = client.messages().create_stream(req).await.unwrap_err();
        assert_eq!(err.status(), Some(http::StatusCode::UNAUTHORIZED));
        assert_eq!(err.request_id(), Some("req_unauth"));
    }

    #[cfg(feature = "streaming")]
    #[tokio::test]
    async fn create_stream_sets_stream_true_in_request_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({"stream": true})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_corpus()),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("x")
            .build()
            .unwrap();
        // The body matcher above is the actual assertion; if `stream: true`
        // wasn't sent, the mock would 404.
        let _ = client.messages().create_stream(req).await.unwrap();
    }
}
