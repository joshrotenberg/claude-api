//! Document + citations example: send a short inline text document and
//! ask Claude to cite where it found the answer.
//!
//! For a real PDF, swap [`ContentBlock::document_text`] for
//! [`ContentBlock::document_url`] (or a base64-encoded
//! [`DocumentSource::Base64`]).
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example document
//! ```

use claude_api::Client;
use claude_api::messages::{ContentBlock, CreateMessageRequest, KnownBlock, MessageContent};
use claude_api::types::ModelId;

const DOC_TEXT: &str = "\
claude-api is a type-safe Rust client for the Anthropic HTTP API.
It targets v0.1 with Messages and Models endpoints, plus streaming and a
configurable retry policy that honors the Retry-After header. Every error
surfaces the request-id from the response. Bedrock and Vertex auth are
planned for v0.5.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set in the environment")?;
    let client = Client::new(api_key);

    let user_turn = MessageContent::Blocks(vec![
        ContentBlock::document_text(DOC_TEXT, Some("claude-api summary")),
        ContentBlock::text(
            "What does claude-api do, and what version supports Bedrock auth? \
             Cite the document.",
        ),
    ]);

    let request = CreateMessageRequest::builder()
        .model(ModelId::SONNET_4_6)
        .max_tokens(512)
        .user(user_turn)
        .build()?;

    let response = client.messages().create(request).await?;

    for block in &response.content {
        if let ContentBlock::Known(KnownBlock::Text {
            text, citations, ..
        }) = block
        {
            println!("{text}");
            if let Some(cites) = citations
                && !cites.is_empty()
            {
                println!("[{} citation(s)]", cites.len());
                for c in cites {
                    let kind = c.type_tag().unwrap_or("?");
                    let title = c.title().unwrap_or("(untitled)");
                    let text = c.cited_text().unwrap_or("");
                    println!("  - [{kind}] {title}: \"{text}\"");
                }
            }
            println!();
        }
    }
    Ok(())
}
