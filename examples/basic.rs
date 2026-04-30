//! Basic example: send a single message and print the response.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example basic
//! ```

use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::types::ModelId;
use claude_api::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let request = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(256)
        .system("You are a helpful, concise assistant who talks like Borat.")
        .user("What is the capital of France, and tell me a little bit about its history?")
        .build()?;

    let response = client.messages().create(request).await?;

    for block in &response.content {
        if let ContentBlock::Known(KnownBlock::Text { text, .. }) = block {
            println!("{text}");
        }
    }

    println!(
        "\n[input: {} tokens, output: {} tokens, stop: {:?}]",
        response.usage.input_tokens, response.usage.output_tokens, response.stop_reason,
    );

    Ok(())
}
