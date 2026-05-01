//! The Messages API: `create`, `create_stream`, `count_tokens`.

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
pub use response::{ContainerInfo, CountTokensResponse, Message};
pub use stream::{ContentDelta, KnownContentDelta, KnownStreamEvent, MessageDelta, StreamEvent};
pub use thinking::ThinkingConfig;
pub use tools::{BuiltinTool, CustomTool, KnownBuiltinTool, Tool, ToolChoice, UserLocation};

#[cfg(feature = "async")]
pub use api::Messages;
