//! Define a tool with `#[derive(Tool)]`.
//!
//! The struct's fields are the tool input; the derive pulls the schema
//! from `JsonSchema`, the name from the type, and the description from
//! the doc comment. The user's `run` method supplies the behavior.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example derived_tool \
//!   --features async,derive,conversation,schemars-tools
//! ```

use claude_api::Client;
use claude_api::conversation::Conversation;
use claude_api::derive::Tool;
use claude_api::messages::{ContentBlock, KnownBlock};
use claude_api::tool_dispatch::{RunOptions, ToolError, ToolRegistry};
use claude_api::types::ModelId;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

/// Get the current weather for a city.
#[derive(Deserialize, JsonSchema, Tool)]
struct GetWeather {
    /// Name of the city.
    city: String,
}

impl GetWeather {
    #[allow(clippy::unused_async)]
    async fn run(self) -> Result<Value, ToolError> {
        Ok(json!({"temp_f": 72, "city": self.city, "summary": "clear"}))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let mut registry = ToolRegistry::new();
    registry.register_tool(GetWeather::tool());

    let mut convo = Conversation::new(ModelId::SONNET_4_6, 1024);
    convo.push_user("What's the weather in San Francisco?");

    let final_msg = client
        .run(
            &mut convo,
            &registry,
            RunOptions::default().max_iterations(8),
        )
        .await?;
    for block in &final_msg.content {
        if let ContentBlock::Known(KnownBlock::Text { text, .. }) = block {
            println!("{text}");
        }
    }
    Ok(())
}
