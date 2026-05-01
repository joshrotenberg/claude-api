//! Streaming with callback hooks.
//!
//! Shows the `on_*` builder methods on [`EventStream`] -- handlers fire as
//! the stream is driven inside [`EventStream::aggregate`], which returns the
//! fully reconstructed [`Message`] at the end. Compared to the raw
//! `streaming` example, you get incremental observability without manually
//! pattern-matching on stream events.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example streaming_callbacks
//! ```

use claude_api::Client;
use claude_api::messages::CreateMessageRequest;
use claude_api::types::ModelId;
use std::io::Write;
use std::sync::{Arc, Mutex};

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

    let stream = client.messages().create_stream(request).await?;

    let chunk_count = Arc::new(Mutex::new(0u32));
    let count_for_handler = Arc::clone(&chunk_count);

    let message = stream
        .on_text_delta(move |chunk| {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(chunk.as_bytes());
            let _ = stdout.flush();
            *count_for_handler.lock().unwrap() += 1;
        })
        .on_message_stop(|usage| {
            eprintln!(
                "\n\n[message_stop] input={} output={}",
                usage.input_tokens, usage.output_tokens,
            );
        })
        .aggregate()
        .await?;

    eprintln!(
        "[summary] {} text-delta callbacks; final content blocks: {}",
        chunk_count.lock().unwrap(),
        message.content.len()
    );

    Ok(())
}
