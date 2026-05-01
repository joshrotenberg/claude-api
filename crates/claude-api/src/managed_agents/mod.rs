//! Anthropic Managed Agents API (preview).
//!
//! Managed Agents lets you provision long-running agent sessions backed by
//! Anthropic-managed compute environments, persistent memory stores, and
//! credential vaults. Each session references a versioned agent and an
//! environment, drives a [stream of events](crate::managed_agents::events),
//! and may produce outputs as files.
//!
//! # Beta status
//!
//! All requests in this module require the `managed-agents-2026-04-01`
//! beta header, which the SDK adds automatically. Outcomes additionally
//! require `managed-agents-2026-04-01-research-preview`.
//!
//! **The API surface is in preview and will change.** Field names,
//! request shapes, and resource relationships are not yet stable. We
//! follow the broader claude-api forward-compatibility contract: every
//! union deserializes into `Other(Value)` when the wire `type` tag is
//! unknown, so brand-new variants don't break the build. New `Known`
//! variants are minor bumps that may require sweeping `Other` matches.
//!
//! Gated on the `managed-agents-preview` feature.
//!
//! # Layout
//!
//! - [`sessions`] -- create, retrieve, list, archive, delete sessions;
//!   send and stream events.
//! - [`vaults`] -- credential vaults for MCP authentication.
//! - [`memory_stores`] -- persistent memory across sessions.
//! - [`agents`] -- agent definitions (create only in this version; full
//!   CRUD lands once docs are available).
//!
//! # Create a session and send a message
//!
//! ```no_run
//! use claude_api::{Client,
//!     managed_agents::sessions::{AgentRef, CreateSessionRequest},
//!     managed_agents::events::OutgoingUserEvent};
//! # async fn run() -> Result<(), claude_api::Error> {
//! let client = Client::new(std::env::var("ANTHROPIC_API_KEY").unwrap());
//! let ma = client.managed_agents();
//! let session = ma.sessions().create(
//!     CreateSessionRequest::builder()
//!         .agent(AgentRef::latest("agt_..."))
//!         .build()?,
//! ).await?;
//! let sessions = ma.sessions();
//! sessions.events(session.id)
//!     .send(&[OutgoingUserEvent::message("Hello!")])
//!     .await?;
//! # Ok(())
//! # }
//! ```

#![cfg(feature = "managed-agents-preview")]
#![cfg_attr(docsrs, doc(cfg(feature = "managed-agents-preview")))]

use crate::client::Client;

pub mod agents;
pub mod environments;
pub mod events;
pub mod memory_stores;
pub mod resources;
pub mod sessions;
pub mod threads;
pub mod vaults;

/// Top-level namespace handle for the Managed Agents API.
///
/// Obtained via [`Client::managed_agents`].
pub struct ManagedAgents<'a> {
    client: &'a Client,
}

impl<'a> ManagedAgents<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Sessions sub-namespace.
    #[must_use]
    pub fn sessions(&self) -> sessions::Sessions<'a> {
        sessions::Sessions::new(self.client)
    }

    /// Vaults sub-namespace (per-user MCP credential vaults).
    #[must_use]
    pub fn vaults(&self) -> vaults::Vaults<'a> {
        vaults::Vaults::new(self.client)
    }

    /// Memory stores sub-namespace (persistent memory across sessions).
    #[must_use]
    pub fn memory_stores(&self) -> memory_stores::MemoryStores<'a> {
        memory_stores::MemoryStores::new(self.client)
    }

    /// Agents sub-namespace (currently `create` only).
    #[must_use]
    pub fn agents(&self) -> agents::Agents<'a> {
        agents::Agents::new(self.client)
    }

    /// Environments sub-namespace (full CRUD + archive).
    #[must_use]
    pub fn environments(&self) -> environments::Environments<'a> {
        environments::Environments::new(self.client)
    }
}

/// Beta header value required on every Managed Agents API request.
pub(crate) const MANAGED_AGENTS_BETA: &str = "managed-agents-2026-04-01";

/// Additional beta header value required for research-preview features
/// like outcomes. Add **alongside** [`MANAGED_AGENTS_BETA`]. Opt in
/// via [`Sessions::with_research_preview`](sessions::Sessions::with_research_preview).
pub(crate) const MANAGED_AGENTS_RESEARCH_PREVIEW_BETA: &str =
    "managed-agents-2026-04-01-research-preview";

/// Pick the right beta-header slice for a Managed Agents request.
///
/// Returns the base header on its own, or both headers when the
/// caller has opted into research-preview features (outcomes via
/// `user.define_outcome` events, span outcome events, and the
/// `Session.outcome_evaluations` field).
pub(crate) const fn betas(research_preview: bool) -> &'static [&'static str] {
    if research_preview {
        &[MANAGED_AGENTS_BETA, MANAGED_AGENTS_RESEARCH_PREVIEW_BETA]
    } else {
        &[MANAGED_AGENTS_BETA]
    }
}
