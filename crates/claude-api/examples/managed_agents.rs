//! Managed Agents preview: create a session, send a message, and read
//! the events back.
//!
//! Requires a provisioned agent and environment ID. The preview feature
//! must be enabled; see <https://docs.anthropic.com/managed-agents>.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... \
//! CLAUDE_AGENT_ID=agt_... \
//!     cargo run --example managed_agents \
//!     --features managed-agents-preview
//! ```

use claude_api::Client;
use claude_api::managed_agents::events::OutgoingUserEvent;
use claude_api::managed_agents::sessions::{AgentRef, CreateSessionRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key =
        std::env::var("ANTHROPIC_API_KEY").map_err(|_| "ANTHROPIC_API_KEY must be set")?;
    let agent_id = std::env::var("CLAUDE_AGENT_ID")
        .map_err(|_| "CLAUDE_AGENT_ID must be set (e.g. agt_...)")?;

    let client = Client::new(api_key);
    let ma = client.managed_agents();

    // Create a session against the latest version of the agent.
    let session = ma
        .sessions()
        .create(
            CreateSessionRequest::builder()
                .agent(AgentRef::latest(&agent_id))
                .build()?,
        )
        .await?;
    println!(
        "Session {} created (status: {:?})",
        session.id, session.status
    );

    // Send the initial user message.
    let sessions = ma.sessions();
    let events_handle = sessions.events(session.id.clone());
    events_handle
        .send(&[OutgoingUserEvent::message(
            "Hello! Summarize what you can do in two sentences.",
        )])
        .await?;
    println!("User message sent.");

    // List events produced so far (poll; for real workloads use .stream()).
    let page = events_handle.list().await?;
    println!("\nEvents received: {}", page.data.len());
    for ev in &page.data {
        if let Some(tag) = ev.type_tag() {
            println!("  {tag}");
        }
    }

    Ok(())
}
