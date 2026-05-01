//! Integration tests for the synchronous (`blocking`) API surface.
//!
//! Requires both `sync` (the API under test) and `async` (wiremock and
//! tokio for the mock server setup). Runs as part of `cargo test --all-features`.

#![cfg(all(feature = "sync", feature = "async"))]

mod common;

use claude_api::blocking::Client;
use claude_api::messages::{ContentBlock, CountTokensRequest, CreateMessageRequest, KnownBlock};
use claude_api::models::ListModelsParams;
use claude_api::types::{ModelId, StopReason};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn blocking_messages_create_round_trips_a_message() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(common::load_fixture_json("message_response.json")),
        )
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let resp = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .build()
            .unwrap();
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(64)
            .user("what is the capital of france?")
            .build()
            .unwrap();
        client.messages().create(req)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(resp.id, "msg_integration_01");
    assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
    assert!(matches!(
        &resp.content[0],
        ContentBlock::Known(KnownBlock::Text { .. })
    ));
}

#[tokio::test]
async fn blocking_models_list_decodes_page_fixture() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(common::load_fixture_json("models_page.json")),
        )
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let page = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .build()
            .unwrap();
        client.models().list(ListModelsParams::default())
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(page.data.len(), 3);
    assert_eq!(page.data[0].id, ModelId::OPUS_4_7);
}

#[tokio::test]
async fn blocking_error_response_propagates_request_id() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("request-id", "req_blk_rl")
                .set_body_json(common::load_fixture_json("error_rate_limit.json")),
        )
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let err = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .retry(claude_api::retry::RetryPolicy::none())
            .build()
            .unwrap();
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("x")
            .build()
            .unwrap();
        client.messages().create(req)
    })
    .await
    .unwrap()
    .unwrap_err();

    assert_eq!(err.status(), Some(http::StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(err.request_id(), Some("req_blk_rl"));
}

#[tokio::test]
async fn blocking_messages_count_tokens_decodes_response() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages/count_tokens"))
        .and(body_partial_json(json!({
            "model": "claude-haiku-4-5-20251001",
            "messages": [{"role": "user", "content": "x"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"input_tokens": 11})))
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let resp = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .build()
            .unwrap();
        let req = CountTokensRequest::builder()
            .model(ModelId::HAIKU_4_5)
            .user("x")
            .build()
            .unwrap();
        client.messages().count_tokens(req)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(resp.input_tokens, 11);
}

#[tokio::test]
async fn blocking_messages_create_with_beta_attaches_per_request_beta_header() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header_exists("anthropic-beta"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(common::load_fixture_json("message_response.json")),
        )
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .build()
            .unwrap();
        let req = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(8)
            .user("x")
            .build()
            .unwrap();
        client
            .messages()
            .create_with_beta(req, &["computer-use-2025-01-24"])
    })
    .await
    .unwrap()
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
async fn blocking_messages_count_tokens_with_beta_attaches_header() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages/count_tokens"))
        .and(header("anthropic-beta", "token-counting-2024-11-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"input_tokens": 3})))
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let resp = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .build()
            .unwrap();
        let req = CountTokensRequest::builder()
            .model(ModelId::HAIKU_4_5)
            .user("y")
            .build()
            .unwrap();
        client
            .messages()
            .count_tokens_with_beta(req, &["token-counting-2024-11-01"])
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(resp.input_tokens, 3);
}

#[tokio::test]
async fn blocking_models_get_decodes_single_model() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models/claude-sonnet-4-6"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "claude-sonnet-4-6",
            "type": "model",
            "display_name": "Claude Sonnet 4.6",
            "created_at": "2026-01-01T00:00:00Z"
        })))
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let model = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .build()
            .unwrap();
        client.models().get("claude-sonnet-4-6")
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(model.id, ModelId::SONNET_4_6);
}

#[tokio::test]
async fn blocking_models_list_all_pages_until_exhausted() {
    let mock = MockServer::start().await;
    // Page 1: has_more=true, last_id="model_1"
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(wiremock::matchers::query_param_is_missing("after_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": "claude-opus-4-7", "type": "model", "display_name": "Claude Opus 4.7", "created_at": "2026-01-01T00:00:00Z"},
                {"id": "claude-sonnet-4-6", "type": "model", "display_name": "Claude Sonnet 4.6", "created_at": "2026-01-01T00:00:00Z"}
            ],
            "has_more": true,
            "first_id": "claude-opus-4-7",
            "last_id": "claude-sonnet-4-6"
        })))
        .mount(&mock)
        .await;
    // Page 2: has_more=false
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(wiremock::matchers::query_param("after_id", "claude-sonnet-4-6"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": "claude-haiku-4-5-20251001", "type": "model", "display_name": "Claude Haiku 4.5", "created_at": "2026-01-01T00:00:00Z"}
            ],
            "has_more": false,
            "first_id": "claude-haiku-4-5-20251001",
            "last_id": "claude-haiku-4-5-20251001"
        })))
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let all = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .build()
            .unwrap();
        client.models().list_all()
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, ModelId::OPUS_4_7);
    assert_eq!(all[2].id, ModelId::HAIKU_4_5);
}

#[tokio::test]
async fn blocking_models_get_propagates_404_with_request_id() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models/no-such-model"))
        .respond_with(
            ResponseTemplate::new(404)
                .insert_header("request-id", "req_blk_404")
                .set_body_json(json!({
                    "type": "error",
                    "error": {"type": "not_found_error", "message": "model not found"}
                })),
        )
        .mount(&mock)
        .await;

    let base_url = mock.uri();
    let err = tokio::task::spawn_blocking(move || {
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(base_url)
            .retry(claude_api::retry::RetryPolicy::none())
            .build()
            .unwrap();
        client.models().get("no-such-model")
    })
    .await
    .unwrap()
    .unwrap_err();

    assert_eq!(err.status(), Some(http::StatusCode::NOT_FOUND));
    assert_eq!(err.request_id(), Some("req_blk_404"));
}
