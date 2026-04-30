//! Manual tool-use loop: drives `messages.create` until the model stops
//! requesting tools.
//!
//! No `ToolRegistry` is used here -- that's an ergonomic helper landing in
//! v0.2. v0.1 callers wire the loop themselves; this example shows the
//! shape.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example tool_use
//! ```

use claude_api::messages::{
    ContentBlock, CreateMessageRequest, CustomTool, KnownBlock, MessageInput, Tool,
    ToolResultContent,
};
use claude_api::types::{ModelId, StopReason};
use claude_api::Client;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let weather_tool = Tool::Custom(
        CustomTool::new(
            "get_weather",
            json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string", "description": "City name"}
                },
                "required": ["city"]
            }),
        )
        .description("Get the current weather for a city. Returns a short string."),
    );

    let mut messages = vec![MessageInput::user(
        "What's the weather like in Paris and Tokyo right now?",
    )];

    // Cap the loop so a buggy tool can't lock us in forever.
    for iteration in 1..=8 {
        let request = CreateMessageRequest::builder()
            .model(ModelId::SONNET_4_6)
            .max_tokens(1024)
            .messages(messages.clone())
            .tools(vec![weather_tool.clone()])
            .build()?;

        let response = client.messages().create(request).await?;

        for block in &response.content {
            if let ContentBlock::Known(KnownBlock::Text { text, .. }) = block {
                println!("[assistant#{iteration}] {text}");
            }
        }

        if response.stop_reason != Some(StopReason::ToolUse) {
            break;
        }

        // Append the model's full assistant turn (text + tool_use blocks),
        // then append a user turn carrying tool_result blocks for each call.
        messages.push(MessageInput::assistant(response.content.clone()));

        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for block in &response.content {
            if let ContentBlock::Known(KnownBlock::ToolUse { id, name, input }) = block {
                println!("[tool#{iteration}] {name}({input})");
                let result = run_tool(name, input);
                println!("[result#{iteration}] {result}");
                tool_results.push(ContentBlock::Known(KnownBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: ToolResultContent::Text(result),
                    is_error: None,
                    cache_control: None,
                }));
            }
        }
        messages.push(MessageInput::user(tool_results));
    }

    Ok(())
}

/// Stand-in tool implementation. A real app would call out to a service.
fn run_tool(name: &str, input: &serde_json::Value) -> String {
    match name {
        "get_weather" => {
            let city = input.get("city").and_then(|v| v.as_str()).unwrap_or("?");
            // Mocked response so the example is self-contained.
            format!("It's 72°F and sunny in {city}.")
        }
        other => format!("(no implementation registered for tool '{other}')"),
    }
}
