//! Integration tests for the Messages namespace.

#![cfg(feature = "async")]

mod common;

use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::types::{ModelId, StopReason};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn create_decodes_realistic_response_fixture() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(common::load_fixture_json("message_response.json")),
        )
        .mount(&mock)
        .await;

    let client = common::client_for(&mock);
    let req = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(64)
        .system("be concise")
        .user("what is the capital of france?")
        .build()
        .unwrap();
    let resp = client.messages().create(req).await.unwrap();

    assert_eq!(resp.id, "msg_integration_01");
    assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(resp.model, ModelId::SONNET_4_6);
    assert_eq!(resp.content.len(), 1);
    match &resp.content[0] {
        ContentBlock::Known(KnownBlock::Text { text, .. }) => {
            assert_eq!(text, "The capital of France is Paris.");
        }
        other => panic!("expected text block, got {other:?}"),
    }
}

#[tokio::test]
async fn create_serializes_request_with_required_fields() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hello"}]
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(common::load_fixture_json("message_response.json")),
        )
        .mount(&mock)
        .await;

    let client = common::client_for(&mock);
    let req = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(100)
        .user("hello")
        .build()
        .unwrap();
    let _ = client.messages().create(req).await.unwrap();
}

#[tokio::test]
async fn rate_limit_error_propagates_with_request_id_and_retry_after() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("request-id", "req_integration_rl")
                .insert_header("retry-after", "3")
                .set_body_json(common::load_fixture_json("error_rate_limit.json")),
        )
        // Match every retry attempt the client makes -- the fixture client uses
        // RetryPolicy::default which can retry 429s; the mock answering each
        // attempt with the same 429 forces eventual give-up.
        .mount(&mock)
        .await;

    // Use a no-retry policy so the test stays fast.
    let client = claude_api::Client::builder()
        .api_key("sk-ant-x")
        .base_url(mock.uri())
        .retry(claude_api::retry::RetryPolicy::none())
        .build()
        .unwrap();

    let req = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(8)
        .user("x")
        .build()
        .unwrap();
    let err = client.messages().create(req).await.unwrap_err();

    assert_eq!(err.status(), Some(http::StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(err.request_id(), Some("req_integration_rl"));
    assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(3)));
}
