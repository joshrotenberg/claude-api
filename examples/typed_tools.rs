//! Schemars-typed tool registration: derive the input schema from a
//! `JsonSchema`-deriving Rust struct; the registry deserializes the
//! model's input into the typed `Args` before calling.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example typed_tools \
//!     --features conversation,schemars-tools
//! ```

use claude_api::conversation::Conversation;
use claude_api::messages::{ContentBlock, KnownBlock};
use claude_api::tool_dispatch::{RunOptions, ToolRegistry};
use claude_api::types::ModelId;
use claude_api::Client;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(JsonSchema, Deserialize)]
#[allow(dead_code)]
struct WeatherArgs {
    /// The city to look up.
    city: String,
    /// Temperature units. Defaults to "F" if omitted.
    #[serde(default)]
    units: Option<String>,
}

#[derive(JsonSchema, Deserialize)]
#[allow(dead_code)]
struct TimeArgs {
    /// IANA timezone identifier (e.g. "America/Los_Angeles").
    timezone: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let mut registry = ToolRegistry::new();
    registry.register_typed_described::<WeatherArgs, _, _>(
        "get_weather",
        "Get the current weather for a city.",
        |args| async move {
            let units = args.units.unwrap_or_else(|| "F".into());
            Ok(json!({"city": args.city, "temp": 22, "units": units}))
        },
    );
    registry.register_typed_described::<TimeArgs, _, _>(
        "get_time",
        "Get the current local time for a timezone.",
        |args| async move { Ok(json!({"timezone": args.timezone, "iso": "2026-04-30T14:00:00"})) },
    );

    let mut convo = Conversation::new(ModelId::SONNET_4_6, 1024)
        .system("You are a concise assistant. Use tools when the user asks for live info.");
    convo.push_user("What's the weather in Berlin in Celsius and the local time in NYC?");

    let final_msg = client
        .run(&mut convo, &registry, RunOptions::default().max_iterations(8))
        .await?;

    for block in &final_msg.content {
        if let ContentBlock::Known(KnownBlock::Text { text, .. }) = block {
            println!("{text}");
        }
    }
    Ok(())
}
