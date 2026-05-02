//! AWS Bedrock auth: sign requests with sigv4 instead of an API key.
//!
//! The example reads AWS credentials from the standard environment
//! variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
//! `AWS_SESSION_TOKEN`, `AWS_REGION`) and sends one message through the
//! Bedrock Anthropic endpoint.
//!
//! ```sh
//! AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_REGION=us-east-1 \
//!     cargo run --example bedrock \
//!     --features bedrock --no-default-features --features async,rustls,streaming,bedrock
//! ```
//!
//! Note: Bedrock Anthropic model IDs use the `anthropic.` prefix, e.g.
//! `anthropic.claude-sonnet-4-6-20251001-v1:0`.

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

    // The Bedrock base URL routes to the regional endpoint.
    let base_url = format!("https://bedrock-runtime.{region}.amazonaws.com");

    let client = Client::builder()
        .signer(Arc::new(signer))
        .base_url(base_url)
        .build()?;

    // Bedrock model IDs use the full Amazon resource name format.
    let model = "anthropic.claude-3-5-sonnet-20241022-v2:0";

    let request = CreateMessageRequest::builder()
        .model(model)
        .max_tokens(256)
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
