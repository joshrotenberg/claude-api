//! Multi-turn conversation example. Demonstrates `Conversation` keeping
//! turn-by-turn state, accumulating usage, and computing cost via the
//! bundled `PricingTable`.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example conversation \
//!     --features pricing
//! ```

use claude_api::conversation::Conversation;
use claude_api::messages::{ContentBlock, KnownBlock};
use claude_api::pricing::PricingTable;
use claude_api::types::ModelId;
use claude_api::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let mut convo = Conversation::new(ModelId::SONNET_4_6, 256)
        .system("You are a concise assistant. Limit answers to one sentence.")
        .with_cache_breakpoint_on_system();

    convo.push_user("What is the tallest mountain in the world?");
    let r1 = convo.send(&client).await?;
    println!("[turn 1] {}", first_text(&r1.content));

    convo.push_user("And the second tallest?");
    let r2 = convo.send(&client).await?;
    println!("[turn 2] {}", first_text(&r2.content));

    convo.push_user("Are they in the same mountain range?");
    let r3 = convo.send(&client).await?;
    println!("[turn 3] {}", first_text(&r3.content));

    let pricing = PricingTable::default();
    let total = convo.cumulative_usage();
    let cost = convo.cost(&pricing);

    println!(
        "\n[turns: {}, cumulative input: {} tokens, output: {} tokens, cost: ${:.6}]",
        convo.turn_count(),
        total.input_tokens,
        total.output_tokens,
        cost,
    );
    Ok(())
}

fn first_text(content: &[ContentBlock]) -> &str {
    for block in content {
        if let ContentBlock::Known(KnownBlock::Text { text, .. }) = block {
            return text;
        }
    }
    "(no text)"
}
