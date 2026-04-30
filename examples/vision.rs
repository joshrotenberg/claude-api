//! Vision example: send an image alongside a question.
//!
//! Uses [`ImageSource::Url`] for the smallest possible runnable example;
//! base64-from-disk would work the same way via
//! [`ContentBlock::image_base64`].
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example vision
//! ```

use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock, MessageContent};
use claude_api::types::ModelId;
use claude_api::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    // Public image URL. Replace with your own; PNG / JPEG / GIF / WebP supported.
    // (Anthropic's docs use this same URL in their vision examples.)
    let image_url =
        "https://upload.wikimedia.org/wikipedia/commons/a/a7/Camponotus_flavomarginatus_ant.jpg";

    let user_turn = MessageContent::Blocks(vec![
        ContentBlock::image_url(image_url),
        ContentBlock::text("What's in this image? Be brief."),
    ]);

    let request = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(512)
        .user(user_turn)
        .build()?;

    let response = client.messages().create(request).await?;
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
