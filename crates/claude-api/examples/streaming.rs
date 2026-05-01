//! Streaming example: print text deltas as they arrive.
//!
//! Demonstrates iterating an [`EventStream`] event-by-event. To get the full
//! reconstructed [`Message`] instead, drop the loop and call
//! `stream.aggregate().await?` -- it returns the same payload as the
//! non-streaming `create()` call.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example streaming
//! ```

use claude_api::Client;
use claude_api::messages::{
    ContentDelta, CreateMessageRequest, KnownContentDelta, KnownStreamEvent, StreamEvent,
};
use claude_api::types::ModelId;
use futures_util::StreamExt;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let request = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(512)
        .user("Tell me a short, surprising fact about lighthouses.")
        .build()?;

    let mut stream = client.messages().create_stream(request).await?;

    let mut stdout = std::io::stdout().lock();
    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                delta: ContentDelta::Known(KnownContentDelta::TextDelta { text }),
                ..
            }) => {
                stdout.write_all(text.as_bytes())?;
                stdout.flush()?;
            }
            StreamEvent::Known(KnownStreamEvent::MessageStop) => break,
            _ => {}
        }
    }
    writeln!(stdout)?;

    Ok(())
}
