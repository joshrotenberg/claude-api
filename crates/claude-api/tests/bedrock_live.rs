//! Opt-in live Bedrock integration test.
//!
//! Skipped by default; run via:
//!
//! ```sh
//! CLAUDE_API_BEDROCK_LIVE=1 \
//! AWS_ACCESS_KEY_ID=... \
//! AWS_SECRET_ACCESS_KEY=... \
//! AWS_REGION=us-east-1 \
//! BEDROCK_MODEL=us.anthropic.claude-sonnet-4-6 \
//!   cargo test --no-default-features \
//!     --features async,rustls,streaming,bedrock \
//!     --test bedrock_live -- --nocapture
//! ```
//!
//! Drives a single `Messages::create` call through the typed client
//! against the regional Bedrock endpoint. The `.bedrock()` builder
//! flag rewrites the URL to `/model/{id}/invoke` and rewrites the
//! body to drop `model` + add `anthropic_version`. Cost is one short
//! request -- a few cents at most.
//!
//! Bedrock model access is enrolled per-account in the AWS console;
//! a 403 `AccessDeniedException` here means the model isn't enabled
//! in this account/region. Newer Claude models (Haiku 4.5, Sonnet
//! 4.6, Opus 4.x) require a cross-region *inference profile* ID
//! (prefixed `us.` or `global.`) rather than the bare foundation
//! model ID; sending the bare ID returns a 400 with the message
//! "Invocation of model ID ... with on-demand throughput isn't
//! supported." Some Anthropic models on Bedrock additionally require
//! a one-time use-case form submission (returns a 404 with the
//! message "Model use case details have not been submitted for this
//! account."). Run `aws bedrock list-inference-profiles` to discover
//! what's available in your region.

#![cfg(feature = "bedrock")]

use std::env;
use std::sync::Arc;

use claude_api::Client;
use claude_api::bedrock::{BedrockCredentials, BedrockSigner};
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};

const ENV_LIVE: &str = "CLAUDE_API_BEDROCK_LIVE";

fn skip_unless_opt_in() -> bool {
    env::var(ENV_LIVE).as_deref() != Ok("1")
}

#[tokio::test]
async fn live_bedrock_messages_create() {
    if skip_unless_opt_in() {
        eprintln!("skipped: set {ENV_LIVE}=1 to run");
        return;
    }

    let region = env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());
    let model =
        env::var("BEDROCK_MODEL").unwrap_or_else(|_| "us.anthropic.claude-sonnet-4-6".into());
    let creds = BedrockCredentials::from_env()
        .expect("AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY must be set");

    let base_url = format!("https://bedrock-runtime.{region}.amazonaws.com");
    let client = Client::builder()
        .api_key("placeholder-bedrock-uses-sigv4")
        .base_url(&base_url)
        .signer(Arc::new(BedrockSigner::new(creds, region)))
        .bedrock()
        .build()
        .expect("client builds");

    let req = CreateMessageRequest::builder()
        .model(model)
        .max_tokens(16)
        .user("Say hello in one word.")
        .build()
        .expect("build request");

    let resp = client
        .messages()
        .create(req)
        .await
        .expect("Bedrock InvokeModel succeeds");

    eprintln!("Bedrock response: {resp:#?}");

    assert_eq!(resp.kind, "message");
    assert!(!resp.content.is_empty(), "content should not be empty");
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::Known(KnownBlock::Text { .. }))),
        "expected at least one text block",
    );
    assert!(
        resp.usage.input_tokens > 0,
        "input_tokens={}",
        resp.usage.input_tokens,
    );
    assert!(
        resp.usage.output_tokens > 0,
        "output_tokens={}",
        resp.usage.output_tokens,
    );
}
