//! Full round-trip: wiremock upstream → recorder → cassette → replay.

use claude_api::Client;
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::models::ListModelsParams;
use claude_api::types::ModelId;
use claude_api_test::{Cassette, Recorder, RecorderConfig, mount_cassette};
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fake_response() -> serde_json::Value {
    json!({
        "id": "msg_recorded",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "hello from upstream"}],
        "model": "claude-sonnet-4-6",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 5, "output_tokens": 3}
    })
}

#[tokio::test]
async fn recorder_forwards_request_and_writes_cassette_then_replays() {
    // 1. Stand up a wiremock server playing the role of the real API.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(json!({"model": "claude-sonnet-4-6"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(fake_response())
                .insert_header("request-id", "req_upstream_1"),
        )
        .mount(&upstream)
        .await;

    // 2. Boot the recorder pointing at the wiremock upstream.
    let tmp = tempfile_path("cassette_rt.jsonl");
    let recorder = Recorder::start(RecorderConfig {
        upstream: upstream.uri(),
        cassette_path: tmp.clone(),
        ..Default::default()
    })
    .await
    .expect("recorder starts");

    // 3. Build a Client pointed at the recorder's URL.
    let client = Client::builder()
        .api_key("sk-ant-real-secret")
        .base_url(recorder.url())
        .build()
        .unwrap();

    let req = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(64)
        .user("hi from test")
        .build()
        .unwrap();
    let live = client.messages().create(req.clone()).await.unwrap();
    assert_eq!(live.id, "msg_recorded");

    recorder.shutdown().await.unwrap();

    // 4. Load the cassette and assert what landed on disk.
    let cassette = Cassette::from_path(&tmp).await.expect("cassette loads");
    assert_eq!(cassette.len(), 1);
    let entry = &cassette.exchanges()[0];
    assert_eq!(entry.method, "POST");
    assert_eq!(entry.path, "/v1/messages");
    assert_eq!(entry.status, 200);
    assert_eq!(entry.response["id"], "msg_recorded");
    // Auth header redacted; non-auth header preserved.
    assert!(
        entry
            .headers
            .iter()
            .any(|(k, v)| k == "request-id" && v == "req_upstream_1"),
        "expected request-id header in cassette",
    );
    // Bodies preserved as decoded JSON.
    let recorded_req = entry
        .request
        .as_ref()
        .expect("recorded request body should be present");
    assert_eq!(recorded_req["model"], "claude-sonnet-4-6");

    // 5. Replay: mount the cassette on a fresh wiremock; the same
    // request should produce the same response without touching the
    // upstream.
    let replay_server = MockServer::start().await;
    mount_cassette(&replay_server, &cassette).await;
    let replay_client = Client::builder()
        .api_key("sk-ant-test")
        .base_url(replay_server.uri())
        .build()
        .unwrap();
    let replayed = replay_client.messages().create(req).await.unwrap();
    assert_eq!(replayed.id, "msg_recorded");
    match &replayed.content[0] {
        ContentBlock::Known(KnownBlock::Text { text, .. }) => {
            assert_eq!(text, "hello from upstream");
        }
        other => panic!("expected text block, got {other:?}"),
    }

    let _ = std::fs::remove_file(&tmp);
}

#[tokio::test]
async fn recorder_redacts_auth_headers_by_default() {
    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": [], "has_more": false}))
                // Pretend the upstream echoed an auth header back; it
                // shouldn't end up in the cassette.
                .insert_header("x-api-key", "sk-ant-leaked-secret")
                .insert_header("x-trace", "trace_123"),
        )
        .mount(&upstream)
        .await;

    let tmp = tempfile_path("cassette_redact.jsonl");
    let recorder = Recorder::start(RecorderConfig {
        upstream: upstream.uri(),
        cassette_path: tmp.clone(),
        ..Default::default()
    })
    .await
    .unwrap();

    let client = Client::builder()
        .api_key("sk-ant-very-secret")
        .base_url(recorder.url())
        .build()
        .unwrap();
    let _ = client
        .models()
        .list(ListModelsParams::default())
        .await
        .unwrap();
    recorder.shutdown().await.unwrap();

    let cassette = Cassette::from_path(&tmp).await.unwrap();
    let entry = &cassette.exchanges()[0];

    let header_keys: Vec<&str> = entry.headers.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        !header_keys.contains(&"x-api-key"),
        "auth header should be redacted: {header_keys:?}"
    );
    assert!(
        header_keys.contains(&"x-trace"),
        "non-auth header should pass through: {header_keys:?}"
    );

    let serialized = std::fs::read_to_string(&tmp).unwrap();
    assert!(
        !serialized.contains("sk-ant-very-secret") && !serialized.contains("sk-ant-leaked-secret"),
        "cassette should not contain any secret-shaped strings",
    );

    let _ = std::fs::remove_file(&tmp);
}

/// Full SSE round-trip: wiremock upstream serving `text/event-stream` ->
/// recorder buffers + writes cassette -> cassette replayed via wiremock ->
/// `Client::create_stream` drives the replayed SSE and aggregates the
/// final message.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn recorder_captures_and_replays_sse_response() {
    use claude_api::messages::CreateMessageRequest;
    use claude_api::types::ModelId;

    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_sse_rt\",\"type\":\"message\",",
        "\"role\":\"assistant\",\"content\":[],\"model\":\"claude-haiku-4-5-20251001\",",
        "\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n",
        "\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n",
        "\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
        "\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}\n",
        "\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n",
        "\n",
    );

    // 1. Stand up a wiremock server serving SSE.
    // Use `set_body_raw` so wiremock sends `content-type: text/event-stream`;
    // `set_body_string` would override the content-type to `text/plain`.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(sse_body.as_bytes(), "text/event-stream")
                .insert_header("request-id", "req_sse_upstream"),
        )
        .mount(&upstream)
        .await;

    // 2. Boot the recorder.
    let tmp = tempfile_path("cassette_sse_rt.jsonl");
    let recorder = Recorder::start(RecorderConfig {
        upstream: upstream.uri(),
        cassette_path: tmp.clone(),
        ..Default::default()
    })
    .await
    .expect("recorder starts");

    // 3. Drive `create_stream` through the recorder.
    let client = Client::builder()
        .api_key("sk-ant-real-secret")
        .base_url(recorder.url())
        .build()
        .unwrap();

    let req = CreateMessageRequest::builder()
        .model(ModelId::HAIKU_4_5)
        .max_tokens(8)
        .user("hi streaming")
        .build()
        .unwrap();

    let stream = client.messages().create_stream(req.clone()).await.unwrap();
    let live_msg = stream.aggregate().await.unwrap();
    assert_eq!(live_msg.id, "msg_sse_rt");
    assert_eq!(live_msg.usage.output_tokens, 1);

    recorder.shutdown().await.unwrap();

    // 4. Inspect the cassette.
    let cassette = Cassette::from_path(&tmp).await.expect("cassette loads");
    assert_eq!(cassette.len(), 1);
    let entry = &cassette.exchanges()[0];
    assert_eq!(entry.method, "POST");
    assert_eq!(entry.path, "/v1/messages");
    assert_eq!(entry.status, 200);
    // The SSE body should be stored as a string, not `<N bytes>`.
    let stored = entry
        .response
        .as_str()
        .expect("SSE response should be stored as a JSON string");
    assert!(
        stored.contains("msg_sse_rt"),
        "cassette should contain the message id: {stored:?}",
    );
    assert!(
        stored.contains("message_start"),
        "cassette should contain SSE events: {stored:?}",
    );
    // content-type header must survive into the cassette.
    assert!(
        entry
            .headers
            .iter()
            .any(|(k, v)| k == "content-type" && v.contains("text/event-stream")),
        "content-type: text/event-stream must be in cassette headers",
    );

    // 5. Replay: mount on a fresh wiremock, drive `create_stream` again.
    let replay_server = MockServer::start().await;
    mount_cassette(&replay_server, &cassette).await;
    let replay_client = Client::builder()
        .api_key("sk-ant-test")
        .base_url(replay_server.uri())
        .build()
        .unwrap();

    let stream2 = replay_client.messages().create_stream(req).await.unwrap();
    let replayed_msg = stream2.aggregate().await.unwrap();
    assert_eq!(replayed_msg.id, "msg_sse_rt");
    assert_eq!(replayed_msg.usage.output_tokens, 1);

    match &replayed_msg.content[0] {
        ContentBlock::Known(KnownBlock::Text { text, .. }) => {
            assert_eq!(text, "hello");
        }
        other => panic!("expected text block, got {other:?}"),
    }

    let _ = std::fs::remove_file(&tmp);
}

fn tempfile_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("claude_api_test_{}_{}", std::process::id(), name));
    p
}
