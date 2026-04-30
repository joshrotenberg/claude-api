//! Agent loop example: `Client::run` drives the tool_use loop automatically.
//!
//! Compare with `tool_use.rs` which writes the same loop by hand. This one
//! collapses to a single `client.run(...)` call.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example agent_loop \
//!     --features conversation
//! ```

use claude_api::conversation::Conversation;
use claude_api::messages::{ContentBlock, KnownBlock};
use claude_api::tool_dispatch::{RunOptions, ToolError, ToolRegistry};
use claude_api::types::ModelId;
use claude_api::Client;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let mut registry = ToolRegistry::new();
    registry.register_described(
        "get_weather",
        "Get the current weather for a city. Returns a short string.",
        json!({
            "type": "object",
            "properties": {
                "city": {"type": "string", "description": "City name"}
            },
            "required": ["city"]
        }),
        |input| async move {
            let city = input
                .get("city")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::invalid_input("missing 'city'"))?;
            Ok(json!({"city": city, "temp_f": 72, "conditions": "sunny"}))
        },
    );

    let mut convo = Conversation::new(ModelId::SONNET_4_6, 1024)
        .system("You are a concise assistant.");
    convo.push_user("What's the weather in Paris and Tokyo? Be brief.");

    let options = RunOptions::default()
        .max_iterations(8)
        .on_iteration(|_msg, n| println!("[iter {n}] turn complete"));

    let final_msg = client.run(&mut convo, &registry, options).await?;

    println!();
    for block in &final_msg.content {
        if let ContentBlock::Known(KnownBlock::Text { text, .. }) = block {
            println!("{text}");
        }
    }
    println!(
        "\n[turns: {}, cumulative input: {} / output: {} tokens]",
        convo.turn_count(),
        convo.cumulative_usage().input_tokens,
        convo.cumulative_usage().output_tokens,
    );
    Ok(())
}
