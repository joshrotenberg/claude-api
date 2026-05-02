//! AWS Bedrock auth: sign requests with sigv4 instead of an API key.
//!
//! Reads AWS credentials from the standard environment variables
//! (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional
//! `AWS_SESSION_TOKEN`, `AWS_REGION`) and sends one message through
//! the Bedrock Anthropic endpoint. The typed `Messages` namespace
//! handles the URL/body shape transform when the client is built with
//! `.bedrock()`: the request goes to `POST /model/{id}/invoke` with
//! `anthropic_version` injected and the top-level `model` field
//! stripped (Bedrock takes the model in the URL).
//!
//! ```sh
//! AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_REGION=us-east-1 \
//!     cargo run --example bedrock \
//!     --no-default-features --features async,rustls,streaming,bedrock
//! ```
//!
//! Bedrock Anthropic model IDs use the `anthropic.` prefix, e.g.
//! `anthropic.claude-haiku-4-5-20251001-v1:0`. The exact list of
//! available models depends on your AWS region and account
//! enrollment. Newer Claude models (Haiku 4.5, Sonnet 4.6, Opus
//! 4.x) require a cross-region *inference profile* ID (prefixed
//! `us.` or `global.`) rather than the bare foundation model ID.
//! Run `aws bedrock list-inference-profiles` to discover what's
//! available in your region.

use std::sync::Arc;

use claude_api::Client;
use claude_api::bedrock::{BedrockCredentials, BedrockSigner};
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());

    let creds = BedrockCredentials::from_env()
        .ok_or("AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY must be set")?;
    let signer = BedrockSigner::new(creds, &region);

    let base_url = format!("https://bedrock-runtime.{region}.amazonaws.com");

    let client = Client::builder()
        .api_key("placeholder-bedrock-uses-sigv4")
        .signer(Arc::new(signer))
        .base_url(base_url)
        .bedrock()
        .build()?;

    let model =
        std::env::var("BEDROCK_MODEL").unwrap_or_else(|_| "us.anthropic.claude-sonnet-4-6".into());

    let request = CreateMessageRequest::builder()
        .model(model)
        .max_tokens(64)
        .user("What is 2 + 2?")
        .build()?;

    let response = client.messages().create(request).await?;

    for block in &response.content {
        if let ContentBlock::Known(KnownBlock::Text { text, .. }) = block {
            println!("{text}");
        }
    }
    println!(
        "\n[input: {} tokens, output: {} tokens]",
        response.usage.input_tokens, response.usage.output_tokens,
    );
    Ok(())
}
