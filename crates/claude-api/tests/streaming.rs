//! End-to-end streaming integration tests, driven by the SSE corpus files
//! under `tests/sse_corpus/`.

#![cfg(all(feature = "async", feature = "streaming"))]

mod common;

use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::types::{ModelId, StopReason};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn hello_world_corpus_aggregates_to_message() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(common::load_sse_corpus("hello_world.sse")),
        )
        .mount(&mock)
        .await;

    let client = common::client_for(&mock);
    let req = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(8)
        .user("hi")
        .build()
        .unwrap();

    let stream = client.messages().create_stream(req).await.unwrap();
    let msg = stream.aggregate().await.unwrap();

    assert_eq!(msg.id, "msg_S");
    assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(msg.usage.output_tokens, 2);
    match &msg.content[0] {
        ContentBlock::Known(KnownBlock::Text { text, .. }) => {
            assert_eq!(text, "Hello world");
        }
        other => panic!("expected text block, got {other:?}"),
    }
}

#[tokio::test]
async fn tool_use_partial_json_corpus_reconstructs_typed_input() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(common::load_sse_corpus("tool_use_partial_json.sse")),
        )
        .mount(&mock)
        .await;

    let client = common::client_for(&mock);
    let req = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(64)
        .user("what's the weather in paris in C?")
        .build()
        .unwrap();

    let msg = client
        .messages()
        .create_stream(req)
        .await
        .unwrap()
        .aggregate()
        .await
        .unwrap();

    assert_eq!(msg.stop_reason, Some(StopReason::ToolUse));
    match &msg.content[0] {
        ContentBlock::Known(KnownBlock::ToolUse { name, input, .. }) => {
            assert_eq!(name, "get_weather");
            assert_eq!(input, &serde_json::json!({"city": "Paris", "unit": "C"}));
        }
        other => panic!("expected tool_use block, got {other:?}"),
    }
}
