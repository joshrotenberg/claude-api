//! End-to-end: load a cassette, mount it, drive a real `claude_api::Client`
//! through it.

use claude_api::Client;
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::types::ModelId;
use claude_api_test::{Cassette, RecordedExchange, mount_cassette};
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::MockServer;

fn client_for(server: &MockServer) -> Client {
    Client::builder()
        .api_key("sk-ant-test")
        .base_url(server.uri())
        .build()
        .unwrap()
}

#[tokio::test]
async fn replays_messages_create_from_jsonl_file() {
    let cassette = Cassette::from_path("tests/cassettes/messages_create.jsonl")
        .await
        .expect("cassette loads");
    assert_eq!(cassette.len(), 1);

    let server = MockServer::start().await;
    mount_cassette(&server, &cassette).await;

    let client = client_for(&server);
    let req = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(64)
        .user("hi")
        .build()
        .unwrap();
    let resp = client.messages().create(req).await.unwrap();
    assert_eq!(resp.id, "msg_replay");
    match &resp.content[0] {
        ContentBlock::Known(KnownBlock::Text { text, .. }) => {
            assert_eq!(text, "Hello from cassette");
        }
        other => panic!("expected text block, got {other:?}"),
    }
}

#[tokio::test]
async fn replays_in_memory_cassette_with_request_body_match() {
    // Two exchanges with the same (method, path) but different request
    // bodies; matching disambiguates by body.
    let cassette = Cassette::from_exchanges(vec![
        RecordedExchange::new(
            "POST",
            "/v1/messages",
            200,
            json!({
                "id": "msg_alpha",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "alpha-resp"}],
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 1}
            }),
        )
        .with_request(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 8,
            "messages": [{"role": "user", "content": "alpha"}]
        })),
        RecordedExchange::new(
            "POST",
            "/v1/messages",
            200,
            json!({
                "id": "msg_beta",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "beta-resp"}],
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 1}
            }),
        )
        .with_request(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 8,
            "messages": [{"role": "user", "content": "beta"}]
        })),
    ]);

    let server = MockServer::start().await;
    mount_cassette(&server, &cassette).await;
    let client = client_for(&server);

    let make_req = |text: &str| {
        CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user(text)
            .build()
            .unwrap()
    };

    let r_alpha = client.messages().create(make_req("alpha")).await.unwrap();
    assert_eq!(r_alpha.id, "msg_alpha");

    let r_beta = client.messages().create(make_req("beta")).await.unwrap();
    assert_eq!(r_beta.id, "msg_beta");
}

#[tokio::test]
async fn replays_error_response_with_request_id_header() {
    let cassette = Cassette::from_exchanges(vec![
        RecordedExchange::new(
            "POST",
            "/v1/messages",
            429,
            json!({
                "type": "error",
                "error": {"type": "rate_limit_error", "message": "slow down"}
            }),
        )
        .with_header("request-id", "req_429_x")
        .with_header("retry-after", "30"),
    ])
    .skip_request_match();

    let server = MockServer::start().await;
    mount_cassette(&server, &cassette).await;
    let client = Client::builder()
        .api_key("sk-ant-test")
        .base_url(server.uri())
        // Disable retries so the 429 surfaces immediately.
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
    assert_eq!(err.request_id(), Some("req_429_x"));
    assert_eq!(err.status(), Some(http::StatusCode::TOO_MANY_REQUESTS));
}
