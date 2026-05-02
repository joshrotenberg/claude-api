//! Message Batches API: submit multiple requests, wait for completion, and
//! read results.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example batches
//! ```

use claude_api::Client;
use claude_api::batches::{BatchRequest, WaitOptions};
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock};
use claude_api::types::ModelId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    // Build three independent requests. Each gets a stable custom_id so
    // results can be correlated back to the inputs after the batch ends.
    let requests = vec![
        BatchRequest::new(
            "capital-france",
            CreateMessageRequest::builder()
                .model(ModelId::HAIKU_4_5)
                .max_tokens(64)
                .user("What is the capital of France? One word.")
                .build()?,
        ),
        BatchRequest::new(
            "capital-japan",
            CreateMessageRequest::builder()
                .model(ModelId::HAIKU_4_5)
                .max_tokens(64)
                .user("What is the capital of Japan? One word.")
                .build()?,
        ),
        BatchRequest::new(
            "capital-brazil",
            CreateMessageRequest::builder()
                .model(ModelId::HAIKU_4_5)
                .max_tokens(64)
                .user("What is the capital of Brazil? One word.")
                .build()?,
        ),
    ];

    // Submit -- returns immediately with processing_status: in_progress.
    let batch = client.batches().create(requests).await?;
    println!(
        "Submitted batch {}  (status: {:?})",
        batch.id, batch.processing_status
    );

    // Poll until the batch ends or a 5-minute timeout fires.
    let options = WaitOptions::default().timeout(std::time::Duration::from_secs(300));
    let finished = client.batches().wait_for(&batch.id, options).await?;
    println!("Batch ended  (status: {:?})", finished.processing_status);

    // Fetch all results and print.
    let items = client.batches().results(&batch.id).await?;
    for item in &items {
        match &item.result {
            claude_api::batches::BatchResultPayload::Succeeded { message } => {
                let text = message
                    .content
                    .iter()
                    .find_map(|b| {
                        if let ContentBlock::Known(KnownBlock::Text { text, .. }) = b {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .unwrap_or("(no text)");
                println!("  [{}] => {}", item.custom_id, text.trim());
            }
            claude_api::batches::BatchResultPayload::Errored { error } => {
                println!("  [{}] => ERROR: {:?}", item.custom_id, error);
            }
            _ => {
                println!("  [{}] => canceled or expired", item.custom_id);
            }
        }
    }

    println!("\n[{} results]", items.len());
    Ok(())
}
