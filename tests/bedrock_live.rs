//! Opt-in live Bedrock integration test.
//!
//! Skipped by default; run via:
//!
//! ```sh
//! BEDROCK_INTEGRATION=1 \
//! AWS_ACCESS_KEY_ID=... \
//! AWS_SECRET_ACCESS_KEY=... \
//! BEDROCK_REGION=us-east-1 \
//! BEDROCK_MODEL=anthropic.claude-3-5-sonnet-20240620-v1:0 \
//!   cargo test --features bedrock --test bedrock_live -- --nocapture
//! ```
//!
//! The test issues a single `InvokeModel` call and asserts the request was
//! signed correctly (the response body shape is incidental). Cost is one
//! short request -- a few cents at most.

#![cfg(feature = "bedrock")]

use std::env;
use std::sync::Arc;

use claude_api::auth::RequestSigner;
use claude_api::bedrock::{BedrockCredentials, BedrockSigner};
use claude_api::Client;

fn skip_unless_opt_in() -> bool {
    env::var("BEDROCK_INTEGRATION").as_deref() != Ok("1")
}

#[tokio::test]
async fn live_bedrock_invoke_model() {
    if skip_unless_opt_in() {
        eprintln!("skipped: set BEDROCK_INTEGRATION=1 to run");
        return;
    }

    let region = env::var("BEDROCK_REGION").unwrap_or_else(|_| "us-east-1".into());
    let model = env::var("BEDROCK_MODEL")
        .unwrap_or_else(|_| "anthropic.claude-3-5-sonnet-20240620-v1:0".into());
    let creds = BedrockCredentials::from_env()
        .expect("AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY must be set");
    let signer = Arc::new(BedrockSigner::new(creds, region.clone()));

    let base_url = format!("https://bedrock-runtime.{region}.amazonaws.com");
    let client = Client::builder()
        .api_key("placeholder-bedrock-uses-sigv4")
        .base_url(&base_url)
        .signer(signer)
        .build()
        .expect("client builds");

    // Bedrock InvokeModel: POST /model/{modelId}/invoke. Body uses
    // Bedrock's `anthropic_version` instead of the date-based one and
    // omits the `model` field (which is in the URL).
    let body = serde_json::json!({
        "anthropic_version": "bedrock-2023-05-31",
        "max_tokens": 16,
        "messages": [{"role": "user", "content": "Say hello in one word."}]
    });

    // We don't go through the typed namespace because Bedrock's URL
    // shape differs from the Anthropic API. Use the low-level execute
    // path directly via reqwest.
    let raw = reqwest::Client::new();
    let mut request = raw
        .post(format!("{base_url}/model/{model}/invoke"))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body.to_string())
        .build()
        .expect("request builds");
    let signer = BedrockSigner::new(BedrockCredentials::from_env().unwrap(), region);
    signer.sign(&mut request).expect("sign succeeds");

    let resp = raw.execute(request).await.expect("request sends");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    assert!(
        status.is_success(),
        "Bedrock InvokeModel failed: {status}\n{text}",
    );
    eprintln!("Bedrock response: {text}");

    // Don't actually use `client` for this round-trip; the URL shape
    // mismatch makes that a v0.5 namespace problem. The test is
    // verifying that BedrockSigner produces signatures the real
    // Bedrock service accepts.
    let _ = client;
}
