//! Integration tests for the Models namespace.

#![cfg(feature = "async")]

mod common;

use claude_api::models::ListModelsParams;
use claude_api::types::ModelId;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn list_decodes_full_page_fixture() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(common::load_fixture_json("models_page.json")),
        )
        .mount(&mock)
        .await;

    let client = common::client_for(&mock);
    let page = client
        .models()
        .list(ListModelsParams::default())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 3);
    assert_eq!(page.data[0].id, ModelId::OPUS_4_7);
    assert_eq!(page.data[1].id, ModelId::SONNET_4_6);
    assert_eq!(page.data[2].id, ModelId::HAIKU_4_5);
    assert!(!page.has_more);
}

#[tokio::test]
async fn get_decodes_single_model_fixture() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models/claude-opus-4-7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "type": "model",
            "id": "claude-opus-4-7",
            "display_name": "Claude Opus 4.7",
            "created_at": "2025-12-01T00:00:00Z"
        })))
        .mount(&mock)
        .await;

    let client = common::client_for(&mock);
    let m = client.models().get("claude-opus-4-7").await.unwrap();
    assert_eq!(m.id, ModelId::OPUS_4_7);
    assert_eq!(m.display_name, "Claude Opus 4.7");
}
