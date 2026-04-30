//! Blocking (sync) client example. No tokio runtime, no `.await`.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example blocking \
//!     --no-default-features --features sync,rustls
//! ```

use claude_api::blocking::Client;
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::types::ModelId;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let request = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(128)
        .system("You are concise.")
        .user("In one sentence: what does the Anthropic SDK do?")
        .build()?;

    // No .await -- this blocks the current thread.
    let response = client.messages().create(request)?;

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
