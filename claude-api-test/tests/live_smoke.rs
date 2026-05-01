//! Live API smoke tests for the cheap, safe endpoints.
//!
//! These tests run in two modes:
//!
//! - **Replay mode** (default, runs in CI): loads the committed
//!   cassette at `tests/cassettes/live_<name>.jsonl` and replays it
//!   through wiremock. No network. No API key needed.
//! - **Record mode** (set `CLAUDE_API_LIVE=1` and `ANTHROPIC_API_KEY`):
//!   forwards live HTTP through the [`Recorder`] proxy to
//!   `api.anthropic.com`, captures every exchange, and writes a fresh
//!   cassette to `tests/cassettes/live_<name>.jsonl`. **Costs real
//!   tokens** -- each test is bounded to single-digit `max_tokens`
//!   when applicable.
//!
//! Workflow to refresh a cassette:
//!
//! ```text
//! ANTHROPIC_API_KEY=sk-ant-... CLAUDE_API_LIVE=1 \
//!     cargo test -p claude-api-test --test live_smoke
//! git add claude-api-test/tests/cassettes/live_*.jsonl
//! ```
//!
//! If a cassette is missing in replay mode the test is skipped with
//! an informative message (not failed) so CI stays green for any
//! endpoint we haven't recorded yet.

use std::future::Future;
use std::path::{Path, PathBuf};

use claude_api::messages::{CountTokensRequest, CreateMessageRequest};
use claude_api::models::ListModelsParams;
use claude_api::types::ModelId;
use claude_api::Client;
use claude_api_test::{mount_cassette, Cassette, Recorder, RecorderConfig};
use wiremock::MockServer;

/// Set to `1` to record fresh cassettes against the live API. Without
/// it we replay from the committed JSONL.
const ENV_LIVE: &str = "CLAUDE_API_LIVE";

/// Upstream endpoint we forward to in record mode.
const UPSTREAM: &str = "https://api.anthropic.com";

/// Drive a `claude_api::Client` through either the recorder (record
/// mode) or a wiremock-mounted cassette (replay mode).
///
/// `body` is the test logic; it receives a configured `Client` whose
/// requests either go to `api.anthropic.com` (record) or to a local
/// cassette server (replay). All assertions live inside `body`.
async fn record_or_replay<F, Fut>(name: &str, body: F)
where
    F: FnOnce(Client) -> Fut,
    Fut: Future<Output = ()>,
{
    let cassette_path = cassette_path(name);

    if std::env::var(ENV_LIVE).is_ok() {
        // ---------- Record mode ----------
        let api_key =
            std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY required for live mode");
        let recorder = Recorder::start(RecorderConfig {
            upstream: UPSTREAM.to_owned(),
            cassette_path: cassette_path.clone(),
            ..Default::default()
        })
        .await
        .expect("recorder starts");

        let client = Client::builder()
            .api_key(api_key)
            .base_url(recorder.url())
            .build()
            .expect("client builds");

        body(client).await;
        recorder.shutdown().await.expect("recorder shuts down");
        eprintln!("recorded cassette: {}", cassette_path.display());
    } else {
        // ---------- Replay mode ----------
        if !cassette_path.exists() {
            eprintln!(
                "skipping live_{name}: no cassette at {} \
                 -- run with CLAUDE_API_LIVE=1 + ANTHROPIC_API_KEY to record",
                cassette_path.display()
            );
            return;
        }
        let cassette = Cassette::from_path(&cassette_path)
            .await
            .expect("cassette loads")
            // Live cassettes can have multiple matching requests; relax
            // body matching so order-of-arrival is enough.
            .skip_request_match();
        let server = MockServer::start().await;
        mount_cassette(&server, &cassette).await;
        let client = Client::builder()
            .api_key("sk-ant-test")
            .base_url(server.uri())
            .build()
            .expect("client builds");
        body(client).await;
    }
}

fn cassette_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cassettes")
        .join(format!("live_{name}.jsonl"))
}

// =====================================================================
// Tier 1: cheap, no model invocation
// =====================================================================

#[tokio::test]
async fn live_models_list() {
    record_or_replay("models_list", |client| async move {
        let page = client
            .models()
            .list(ListModelsParams::default())
            .await
            .expect("list models");
        assert!(!page.data.is_empty(), "models list should not be empty");
        // Sanity: every returned model has an ID and a created_at.
        for m in &page.data {
            assert!(!m.id.as_str().is_empty(), "{m:?}");
        }
    })
    .await;
}

#[tokio::test]
async fn live_models_get_sonnet_4_6() {
    record_or_replay("models_get_sonnet_4_6", |client| async move {
        let m = client
            .models()
            .get("claude-sonnet-4-6")
            .await
            .expect("get model");
        assert_eq!(m.id.as_str(), "claude-sonnet-4-6");
        assert_eq!(m.kind, "model");
        assert!(!m.display_name.is_empty());
    })
    .await;
}

#[tokio::test]
async fn live_messages_count_tokens() {
    record_or_replay("messages_count_tokens", |client| async move {
        let req = CountTokensRequest::builder()
            .model(ModelId::SONNET_4_6)
            .user("Tell me a one-line joke about Rust.")
            .build()
            .expect("build count_tokens request");
        let r = client
            .messages()
            .count_tokens(req)
            .await
            .expect("count tokens");
        assert!(r.input_tokens > 0, "expected non-zero token count");
    })
    .await;
}

// =====================================================================
// Tier 2: bounded model invocations
// =====================================================================

#[tokio::test]
async fn live_messages_create_minimal() {
    record_or_replay("messages_create_minimal", |client| async move {
        let req = CreateMessageRequest::builder()
            .model(ModelId::HAIKU_4_5)
            // Bounded -- we don't care what the model says, only that
            // the wire shape decodes correctly.
            .max_tokens(8)
            .user("Reply with a single word.")
            .build()
            .expect("build messages create request");
        let resp = client.messages().create(req).await.expect("create message");
        assert_eq!(resp.kind, "message");
        assert!(!resp.content.is_empty(), "content should not be empty");
        assert!(resp.usage.input_tokens > 0);
        assert!(resp.usage.output_tokens > 0);
    })
    .await;
}
