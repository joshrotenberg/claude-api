//! Type-safe Rust client for the [Anthropic API](https://docs.anthropic.com/).
//!
//! Every documented Anthropic endpoint is reachable through a typed
//! namespace handle off [`Client`]. Forward-compatible by design --
//! unknown content blocks, stream events, citations, and similar
//! discriminated unions round-trip through `Other(Value)` arms so
//! new server-side variants don't break older SDK builds.
//!
//! # Quick start
//!
//! ```no_run
//! use claude_api::{Client, messages::CreateMessageRequest, types::ModelId};
//!
//! # async fn run() -> Result<(), claude_api::Error> {
//! let client = Client::new(std::env::var("ANTHROPIC_API_KEY").unwrap());
//!
//! let resp = client
//!     .messages()
//!     .create(
//!         CreateMessageRequest::builder()
//!             .model(ModelId::SONNET_4_6)
//!             .max_tokens(256)
//!             .user("Hello!")
//!             .build()?,
//!     )
//!     .await?;
//!
//! for block in &resp.content {
//!     if let claude_api::messages::ContentBlock::Known(
//!         claude_api::messages::KnownBlock::Text { text, .. },
//!     ) = block
//!     {
//!         println!("{text}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Module map
//!
//! Endpoints are organized by Anthropic resource. Each namespace
//! handle is reached via a method on [`Client`]:
//!
//! | Module | Namespace | Default? |
//! |---|---|---|
//! | [`messages`] | `client.messages()` | yes |
//! | [`models`] | `client.models()` | yes |
//! | [`batches`] | `client.batches()` | yes |
//! | [`files`] | `client.files()` | yes |
//! | [`skills`] | `client.skills()` | feature `skills` |
//! | [`user_profiles`] | `client.user_profiles()` | feature `user-profiles` |
//! | [`managed_agents`] | `client.managed_agents()` | feature `managed-agents-preview` |
//! | [`admin`] | `client.admin()` | feature `admin` |
//!
//! Cross-cutting machinery:
//!
//! | Module | Purpose |
//! |---|---|
//! | [`auth`] | API-key wrapper + [`auth::RequestSigner`] trait |
//! | [`bedrock`] | AWS sigv4 [`bedrock::BedrockSigner`] (feature `bedrock`) |
//! | [`beta`] | [`BetaHeader`] open-string enum for the `anthropic-beta` header |
//! | [`retry`] | [`retry::RetryPolicy`] honoring `Retry-After` |
//! | [`error`] | [`Error`] with `request_id`, retry classification |
//! | [`pagination`] | [`Paginated`](pagination::Paginated) and [`PaginatedNextPage`](pagination::PaginatedNextPage) |
//! | [`types`] | Shared primitives: [`ModelId`](types::ModelId), [`Role`](types::Role), [`Usage`](types::Usage), [`StopReason`](types::StopReason) |
//! | [`conversation`] | Multi-turn helper with cumulative usage (feature `conversation`) |
//! | [`tool_dispatch`] | [`ToolRegistry`](tool_dispatch::ToolRegistry), agent loop, parallel dispatch, approval gates |
//! | [`pricing`] | [`PricingTable`](pricing::PricingTable) (feature `pricing`) |
//! | [`cost_preview`] | Pre-flight USD estimates (features `async` + `pricing`) |
//! | [`dry_run`] | Render the would-be HTTP request without sending |
//! | [`sse`] | SSE wrapper used by streaming endpoints (feature `streaming`) |
//!
//! # Forward compatibility
//!
//! Every wire-level discriminated union has an `Other` arm:
//!
//! - [`messages::ContentBlock`] -- text / image / tool_use /
//!   thinking / ... / `Other(Value)`
//! - [`messages::KnownBlock`] -- the typed variants
//! - [`messages::stream::StreamEvent`] -- SSE events
//! - [`messages::citation::Citation`] -- citation kinds
//! - [`BetaHeader`] -- known beta header values + `Other(String)`
//!
//! When Anthropic adds a new variant, your code that pattern-matches
//! on `Known(...)` continues to compile; the new variant lands in
//! `Other(...)` until you upgrade. See the **upgrade contract** in
//! the README before bumping past a release that promotes an `Other`
//! to `Known`.
//!
//! # Error handling
//!
//! All endpoint methods return [`Result<T>`](Result) where
//! [`Error`] carries:
//!
//! - HTTP status (when the API responded)
//! - `request-id` (always populated when the server sent one)
//! - Retry classification (`is_retryable()`) and parsed
//!   `Retry-After` (`retry_after()`)
//! - Structured kind ([`error::ApiErrorKind`]) for known
//!   error types
//!
//! The [`retry::RetryPolicy`] applied by the client respects
//! `Retry-After` by default and uses jittered exponential backoff
//! otherwise. Disable retries with `RetryPolicy::none()` if your
//! caller wraps its own retry logic.
//!
//! # Observability
//!
//! Every HTTP request emits a `tracing` span at `debug` level with
//! `method`, `path`, and `request_id` fields. Retries log at `warn`
//! with `attempt`, `retry_in_ms`, and `status`. No env-var gating
//! is required; install a `tracing-subscriber` to capture the
//! events.

#![cfg_attr(docsrs, feature(doc_cfg))]

#[allow(dead_code)]
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";
#[allow(dead_code)]
pub(crate) const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
#[allow(dead_code)]
pub(crate) const USER_AGENT: &str = concat!("claude-api-rs/", env!("CARGO_PKG_VERSION"));

pub mod auth;
pub mod beta;
pub mod error;

#[cfg(feature = "bedrock")]
#[cfg_attr(docsrs, doc(cfg(feature = "bedrock")))]
pub mod bedrock;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod client;

#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod blocking;

#[cfg(any(feature = "async", feature = "sync"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "async", feature = "sync"))))]
pub mod retry;

pub(crate) mod forward_compat;

#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
pub mod sse;

pub mod pagination;

#[cfg(feature = "pricing")]
#[cfg_attr(docsrs, doc(cfg(feature = "pricing")))]
pub mod pricing;

#[cfg(feature = "conversation")]
#[cfg_attr(docsrs, doc(cfg(feature = "conversation")))]
pub mod conversation;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod tool_dispatch;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod dry_run;

#[cfg(all(feature = "async", feature = "pricing"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "async", feature = "pricing"))))]
pub mod cost_preview;

pub mod types;

pub mod batches;
pub mod files;
pub mod messages;
pub mod models;

#[cfg(feature = "managed-agents-preview")]
#[cfg_attr(docsrs, doc(cfg(feature = "managed-agents-preview")))]
pub mod managed_agents;

#[cfg(feature = "admin")]
#[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
pub mod admin;

#[cfg(feature = "skills")]
#[cfg_attr(docsrs, doc(cfg(feature = "skills")))]
pub mod skills;

#[cfg(feature = "user-profiles")]
#[cfg_attr(docsrs, doc(cfg(feature = "user-profiles")))]
pub mod user_profiles;

pub use beta::BetaHeader;
#[cfg(feature = "async")]
pub use client::{Client, ClientBuilder};
pub use error::{Error, Result};

/// Procedural-macro re-exports for the `derive` feature.
///
/// Bring [`Tool`](crate::derive::Tool) into scope to derive
/// [`tool_dispatch::Tool`] on a struct that
/// implements [`serde::Deserialize`] and [`schemars::JsonSchema`].
#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub mod derive {
    pub use claude_api_derive::Tool;
}

/// Implementation detail: re-exports used by macros generated by the
/// `derive` feature. Not part of the public API; do not use directly.
#[cfg(feature = "derive")]
#[doc(hidden)]
pub mod __private {
    pub use async_trait;
    pub use schemars;
    pub use serde_json;
}
