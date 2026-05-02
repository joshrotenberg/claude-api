//! The Messages API.
//!
//! The headline endpoint of the Anthropic API. Build a request via
//! [`CreateMessageRequest::builder`], then send it through the
//! [`Messages`] namespace handle obtained from
//! [`Client::messages`](crate::Client::messages).
//!
//! # Endpoints
//!
//! | Method | Path | Function |
//! |---|---|---|
//! | `POST` | `/v1/messages` | [`Messages::create`] (non-streaming) |
//! | `POST` | `/v1/messages` | [`Messages::create_stream`] (SSE) |
//! | `POST` | `/v1/messages/count_tokens` | [`Messages::count_tokens`] |
//!
//! # Quick start
//!
//! ```no_run
//! use claude_api::{Client, messages::CreateMessageRequest, types::ModelId};
//! # async fn run() -> Result<(), claude_api::Error> {
//! let client = Client::new("sk-ant-...");
//! let resp = client.messages().create(
//!     CreateMessageRequest::builder()
//!         .model(ModelId::SONNET_4_6)
//!         .max_tokens(256)
//!         .system("Be concise.")
//!         .user("What is the capital of France?")
//!         .build()?,
//! ).await?;
//! # Ok(()) }
//! ```
//!
//! # Module layout
//!
//! - [`request`] -- [`CreateMessageRequest`], [`CountTokensRequest`],
//!   builders
//! - [`response`] -- [`Message`], [`CountTokensResponse`],
//!   [`ContainerInfo`]
//! - [`content`] -- [`ContentBlock`] / [`KnownBlock`] union with
//!   forward-compat fallthrough
//! - [`stream`] -- [`StreamEvent`], [`ContentDelta`], the
//!   `EventStream` aggregator + `on_*` callbacks
//! - [`tools`] -- [`Tool`], [`BuiltinTool`], [`CustomTool`],
//!   [`ToolChoice`]
//! - [`cache`] -- [`CacheControl`] for prompt caching
//! - [`thinking`] -- [`ThinkingConfig`] for extended thinking
//! - [`mcp`] -- [`McpServerConfig`] for MCP server invocation
//! - [`citation`] -- typed [`Citation`] enum
//! - [`input`] -- [`MessageInput`], [`SystemPrompt`] helpers
//! - [`metadata`] -- [`MessageMetadata`], [`RequestServiceTier`]
//!
//! For streaming, see [`stream`] for the wire-event types and
//! [`api::Messages::create_stream`] for the namespace method.

pub mod cache;
pub mod citation;
pub mod content;
pub mod input;
pub mod mcp;
pub mod metadata;
pub mod request;
pub mod response;
pub mod stream;
pub mod thinking;
pub mod tools;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod api;

pub use cache::CacheControl;
pub use citation::{Citation, KnownCitation};
pub use content::{
    CitationConfig, ContentBlock, DocumentSource, ImageSource, KnownBlock, ToolResultContent,
};
pub use input::{MessageContent, MessageInput, SystemPrompt};
pub use mcp::{McpServerConfig, McpToolConfiguration};
pub use metadata::{MessageMetadata, RequestServiceTier};
pub use request::{
    CountTokensRequest, CountTokensRequestBuilder, CreateMessageRequest,
    CreateMessageRequestBuilder,
};
pub use response::{
    ClearThinkingEdit, ClearToolUsesEdit, ContainerInfo, ContextEdit, CountTokensResponse,
    KnownContextEdit, KnownStopDetails, Message, RefusalStopDetails, ResponseContextManagement,
    StopDetails,
};
pub use stream::{ContentDelta, KnownContentDelta, KnownStreamEvent, MessageDelta, StreamEvent};
pub use thinking::ThinkingConfig;
pub use tools::{BuiltinTool, CustomTool, KnownBuiltinTool, Tool, ToolChoice, UserLocation};

#[cfg(feature = "async")]
pub use api::Messages;
