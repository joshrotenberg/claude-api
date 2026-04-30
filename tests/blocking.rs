//! Integration tests for the synchronous (`blocking`) API surface.
//!
//! Requires both `sync` (the API under test) and `async` (wiremock and
//! tokio for the mock server setup). Runs as part of `cargo test --all-features`.

#![cfg(all(feature = "sync", feature = "async"))]

mod common;

use claude_api::blocking::Client;
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::models::ListModelsParams;
use claude_api::types::{ModelId, StopReason};
use wiremock::matchers::{method, path};
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
