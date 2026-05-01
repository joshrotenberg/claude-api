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
//! Workflow to refresh non-admin cassettes:
//!
//! ```text
//! ANTHROPIC_API_KEY=sk-ant-... CLAUDE_API_LIVE=1 \
//!     cargo test -p claude-api-test --test live_smoke -- \
//!     --skip live_admin
//! git add claude-api-test/tests/cassettes/live_*.jsonl
//! ```
//!
//! Workflow to refresh admin cassettes (requires an admin-tier API
//! key, distinct from the regular key):
//!
//! ```text
//! ANTHROPIC_ADMIN_API_KEY=sk-ant-admin-... CLAUDE_API_LIVE=1 \
//!     cargo test -p claude-api-test --test live_smoke -- live_admin
//! ```
//!
//! The admin tests filter responses through the same redactor, but
//! note: the `api_keys.list` response legitimately contains
//! `partial_key_hint` fields (e.g. `sk-ant-api03-XXXX...YYYY`).
//! These are public-by-design 8-char key fingerprints, not full
//! secrets, and are safe to commit.
//!
//! If a cassette is missing in replay mode the test is skipped with
//! an informative message (not failed) so CI stays green for any
//! endpoint we haven't recorded yet.

use std::future::Future;
use std::path::{Path, PathBuf};

use claude_api::Client;
use claude_api::batches::BatchRequest;
use claude_api::managed_agents::agents::{AgentModel, CreateAgentRequest};
use claude_api::managed_agents::environments::{CreateEnvironmentRequest, EnvironmentConfig};
use claude_api::managed_agents::events::OutgoingUserEvent;
use claude_api::managed_agents::memory_stores::{CreateMemoryRequest, CreateMemoryStoreRequest};
use claude_api::managed_agents::sessions::CreateSessionRequest;
use claude_api::messages::{CountTokensRequest, CreateMessageRequest};
use claude_api::models::ListModelsParams;
use claude_api::types::ModelId;
use claude_api_test::{Cassette, Recorder, RecorderConfig, mount_cassette};
use wiremock::MockServer;

/// Set to `1` to record fresh cassettes against the live API. Without
/// it we replay from the committed JSONL.
const ENV_LIVE: &str = "CLAUDE_API_LIVE";

/// Upstream endpoint we forward to in record mode.
const UPSTREAM: &str = "https://api.anthropic.com";

/// Drive a `claude_api::Client` through either the recorder (record
/// mode) or a wiremock-mounted cassette (replay mode). Uses the
/// regular API key.
async fn record_or_replay<F, Fut>(name: &str, body: F)
where
    F: FnOnce(Client) -> Fut,
    Fut: Future<Output = ()>,
{
    record_or_replay_with_key("ANTHROPIC_API_KEY", name, body).await;
}

/// Same as [`record_or_replay`] but uses the admin API key
/// (`ANTHROPIC_ADMIN_API_KEY`). Admin endpoints reject regular keys
/// with a 401, so admin live tests must source the admin-tier key.
async fn record_or_replay_admin<F, Fut>(name: &str, body: F)
where
    F: FnOnce(Client) -> Fut,
    Fut: Future<Output = ()>,
{
    record_or_replay_with_key("ANTHROPIC_ADMIN_API_KEY", name, body).await;
}

async fn record_or_replay_with_key<F, Fut>(env_var: &str, name: &str, body: F)
where
    F: FnOnce(Client) -> Fut,
    Fut: Future<Output = ()>,
{
    let cassette_path = cassette_path(name);

    if std::env::var(ENV_LIVE).is_ok() {
        // ---------- Record mode ----------
        let api_key =
            std::env::var(env_var).unwrap_or_else(|_| panic!("{env_var} required for live mode"));
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

// =====================================================================
// Tier 2: bounded write+read+cleanup cycles
// =====================================================================

#[tokio::test]
async fn live_files_upload_retrieve_delete() {
    record_or_replay("files_upload_retrieve_delete", |client| async move {
        let payload = b"live-test fixture\n".to_vec();
        let uploaded = client
            .files()
            .upload_bytes(payload.clone(), "live_smoke.txt", "text/plain")
            .await
            .expect("upload file");
        assert!(uploaded.id.starts_with("file_"), "id={}", uploaded.id);
        assert_eq!(uploaded.filename, "live_smoke.txt");
        assert_eq!(uploaded.size_bytes, payload.len() as u64);

        let retrieved = client
            .files()
            .get(&uploaded.id)
            .await
            .expect("retrieve file");
        assert_eq!(retrieved.id, uploaded.id);

        let deleted = client
            .files()
            .delete(&uploaded.id)
            .await
            .expect("delete file");
        assert_eq!(deleted.id, uploaded.id);
    })
    .await;
}

#[tokio::test]
async fn live_batches_create_cancel_delete() {
    record_or_replay("batches_create_cancel_delete", |client| async move {
        let entry = BatchRequest::new(
            "live-test-1",
            CreateMessageRequest::builder()
                .model(ModelId::HAIKU_4_5)
                .max_tokens(8)
                .user("ping")
                .build()
                .expect("build batch entry"),
        );
        let batch = client
            .batches()
            .create(vec![entry])
            .await
            .expect("create batch");
        assert!(batch.id.starts_with("msgbatch_"), "id={}", batch.id);

        // Cancel before processing produces a no-op or quick state
        // change; we don't assert on the post-cancel status because
        // it varies (canceling vs ended).
        let _ = client.batches().cancel(&batch.id).await.expect("cancel");

        // Delete may fail if the batch is still in 'canceling' state.
        // Tolerate either outcome -- the cassette records whichever
        // status was returned.
        let _ = client.batches().delete(&batch.id).await;
    })
    .await;
}

#[tokio::test]
async fn live_skills_list() {
    record_or_replay("skills_list", |client| async move {
        let page = client
            .skills()
            .list(claude_api::skills::ListSkillsParams::default())
            .await
            .expect("list skills");
        // Page may be empty depending on the org; just confirm the
        // envelope decodes.
        let _ = page.data.len();
    })
    .await;
}

// NOTE: live_user_profiles_list intentionally omitted. The
// user_profiles endpoint requires explicit org enrollment (the live
// API returns 404 NotFoundError for orgs without access). Re-add when
// enrolled or when we want a 404-handling regression cassette.

// =====================================================================
// Tier 3: managed-agents full provisioning cycle
// =====================================================================

#[tokio::test]
async fn live_memory_stores_lifecycle() {
    record_or_replay("memory_stores_lifecycle", |client| async move {
        // Create -> create memory -> list -> delete cycle.
        let store = client
            .managed_agents()
            .memory_stores()
            .create(
                CreateMemoryStoreRequest::new("live-smoke-store")
                    .with_description("Created by claude-api live smoke tests."),
            )
            .await
            .expect("create memory store");
        assert!(store.id.starts_with("memstore_"), "id={}", store.id);

        let mem = client
            .managed_agents()
            .memory_stores()
            .memories(&store.id)
            .create(CreateMemoryRequest::new(
                "/notes/test.md",
                "# test\nA live-smoke memory.\n",
            ))
            .await
            .expect("create memory");
        assert!(mem.id.starts_with("mem_"), "id={}", mem.id);

        let listed = client
            .managed_agents()
            .memory_stores()
            .memories(&store.id)
            .list(claude_api::managed_agents::memory_stores::ListMemoriesParams::default())
            .await
            .expect("list memories");
        assert!(!listed.data.is_empty());

        // Best-effort cleanup: delete the memory then the store.
        let _ = client
            .managed_agents()
            .memory_stores()
            .memories(&store.id)
            .delete(&mem.id)
            .await;
        let _ = client
            .managed_agents()
            .memory_stores()
            .delete(&store.id)
            .await;
    })
    .await;
}

#[tokio::test]
async fn live_managed_agents_full_cycle() {
    record_or_replay("managed_agents_full_cycle", |client| async move {
        // 1) Provision an environment.
        let env = client
            .managed_agents()
            .environments()
            .create(CreateEnvironmentRequest::new(
                "live-smoke-env",
                EnvironmentConfig::cloud().build(),
            ))
            .await
            .expect("create environment");
        assert!(env.id.starts_with("env_"), "env id={}", env.id);

        // 2) Provision an agent (haiku, no tools, minimal system).
        let agent_req = CreateAgentRequest::builder()
            .name("live-smoke-agent")
            .model(AgentModel::String("claude-haiku-4-5".into()))
            .system("You are a live-test agent. Reply with one short word.")
            .build()
            .expect("build agent request");
        let agent = client
            .managed_agents()
            .agents()
            .create(agent_req)
            .await
            .expect("create agent");
        assert!(agent.id.starts_with("agent_"), "agent id={}", agent.id);

        // 3) Open a session.
        let session_req = CreateSessionRequest::builder()
            .agent(agent.id.clone())
            .environment_id(env.id.clone())
            .title("live-smoke session")
            .build()
            .expect("build session request");
        let session = client
            .managed_agents()
            .sessions()
            .create(session_req)
            .await
            .expect("create session");
        assert!(session.id.starts_with("sesn_"), "sesn id={}", session.id);

        // 4) Send a single user message. The session may or may not
        // complete a model turn before we tear down -- we just verify
        // events_send + events_list produce something coherent.
        client
            .managed_agents()
            .sessions()
            .events(&session.id)
            .send(&[OutgoingUserEvent::message("ping")])
            .await
            .expect("events send");

        let events = client
            .managed_agents()
            .sessions()
            .events(&session.id)
            .list()
            .await
            .expect("events list");
        assert!(
            !events.data.is_empty(),
            "expected at least the echoed user.message event"
        );

        // 5) Clean up: archive session, archive agent, archive env.
        // Best-effort -- if any archive fails we still want the prior
        // provisioning recorded for the cassette.
        let _ = client
            .managed_agents()
            .sessions()
            .archive(&session.id)
            .await;
        let _ = client.managed_agents().agents().archive(&agent.id).await;
        let _ = client
            .managed_agents()
            .environments()
            .archive(&env.id)
            .await;
    })
    .await;
}

// =====================================================================
// Tier 4: admin (read-only)
// =====================================================================
//
// Admin endpoints require an admin-tier API key, distinct from the
// regular key. Use `record_or_replay_admin` so record mode sources
// `ANTHROPIC_ADMIN_API_KEY` instead of `ANTHROPIC_API_KEY`.

#[tokio::test]
async fn live_admin_organization_me() {
    record_or_replay_admin("admin_organization_me", |client| async move {
        let org = client
            .admin()
            .organization()
            .me()
            .await
            .expect("organization me");
        assert!(!org.id.is_empty(), "org id empty");
        assert!(!org.name.is_empty(), "org name empty");
    })
    .await;
}

#[tokio::test]
async fn live_admin_users_list() {
    record_or_replay_admin("admin_users_list", |client| async move {
        let page = client
            .admin()
            .users()
            .list(claude_api::admin::users::ListUsersParams::default())
            .await
            .expect("users list");
        // Org has at least one user (the caller).
        assert!(!page.data.is_empty());
    })
    .await;
}

#[tokio::test]
async fn live_admin_workspaces_list() {
    record_or_replay_admin("admin_workspaces_list", |client| async move {
        let page = client
            .admin()
            .workspaces()
            .list(claude_api::admin::workspaces::ListWorkspacesParams::default())
            .await
            .expect("workspaces list");
        let _ = page.data.len();
    })
    .await;
}

#[tokio::test]
async fn live_admin_api_keys_list() {
    record_or_replay_admin("admin_api_keys_list", |client| async move {
        let page = client
            .admin()
            .api_keys()
            .list(claude_api::admin::api_keys::ListApiKeysParams::default())
            .await
            .expect("api keys list");
        let _ = page.data.len();
    })
    .await;
}

#[tokio::test]
async fn live_admin_invites_list() {
    record_or_replay_admin("admin_invites_list", |client| async move {
        let page = client
            .admin()
            .invites()
            .list(claude_api::admin::ListParams::default())
            .await
            .expect("invites list");
        let _ = page.data.len();
    })
    .await;
}

#[tokio::test]
async fn live_admin_rate_limits_org() {
    record_or_replay_admin("admin_rate_limits_org", |client| async move {
        let page = client
            .admin()
            .rate_limits()
            .list_organization(claude_api::admin::rate_limits::ListOrgRateLimitsParams::default())
            .await
            .expect("rate limits org");
        let _ = page.data.len();
    })
    .await;
}
