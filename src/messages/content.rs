//! Content blocks: the building blocks of message bodies and stream deltas.
//!
//! [`ContentBlock`] is the public, forward-compatible enum: it wraps a
//! [`KnownBlock`] for any block type the SDK understands, or a raw
//! [`serde_json::Value`] for any block type it doesn't. This means an SDK
//! older than the API will keep round-tripping payloads instead of panicking
//! on a new variant.
//!
//! # Forward-compat semantics
//!
//! - **Unknown `type` tag** → [`ContentBlock::Other`] preserving the JSON byte-for-byte.
//! - **Known `type` tag with malformed fields** → deserialization error
//!   (we do *not* silently fall through, so genuine bugs surface).
//!
//! ```
//! use claude_api::messages::content::ContentBlock;
//!
//! let json = serde_json::json!({"type": "text", "text": "hi"});
//! let block: ContentBlock = serde_json::from_value(json).unwrap();
//! assert_eq!(block.type_tag(), Some("text"));
//! ```

use serde::{Deserialize, Serialize};

use crate::messages::cache::CacheControl;

/// One block of content within a message.
///
/// Forward-compatible: unknown `type` tags deserialize into [`ContentBlock::Other`]
/// with the raw JSON preserved.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    /// A block whose `type` is recognized by this SDK version.
    Known(KnownBlock),
    /// A block whose `type` is not recognized; the raw JSON is preserved.
    Other(serde_json::Value),
}

/// All content block variants known to this SDK version.
///
/// `#[non_exhaustive]` so that adding a new variant in a future release
/// is not a breaking change for downstream `match` statements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnownBlock {
    /// Plain text content.
    Text {
        /// The text payload.
        text: String,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
        /// Optional citations attached to this text. Modeled as raw JSON for
        /// v0.1; will land as a typed enum once the citations feature is
        /// fully fleshed out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        citations: Option<Vec<serde_json::Value>>,
    },
    /// An image embedded in the message.
    Image {
        /// Where the image bytes come from.
        source: ImageSource,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// A document (e.g. PDF) embedded in the message.
    Document {
        /// Where the document bytes come from.
        source: DocumentSource,
        /// Optional human-readable title used in citation rendering.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Optional citation configuration for this document.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        citations: Option<CitationConfig>,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// A model-emitted request to invoke a tool.
    ToolUse {
        /// Identifier the model assigns to this invocation.
        id: String,
        /// Name of the tool to invoke.
        name: String,
        /// Tool arguments as JSON.
        input: serde_json::Value,
    },
    /// The result of a tool invocation, supplied back to the model.
    ToolResult {
        /// The `id` of the [`KnownBlock::ToolUse`] this result corresponds to.
        tool_use_id: String,
        /// The tool's output.
        content: ToolResultContent,
        /// Whether the tool execution failed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        /// Optional cache breakpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Extended-thinking trace from the model.
    Thinking {
        /// The model's chain-of-thought text.
        thinking: String,
        /// Cryptographic signature over the thinking text.
        signature: String,
    },
    /// A redacted thinking block; only the opaque blob is visible.
    RedactedThinking {
        /// Opaque server-side blob.
        data: String,
    },
    /// A server-side tool invocation initiated by the model.
    ServerToolUse {
        /// Identifier the model assigns to this invocation.
        id: String,
        /// Server tool name (e.g. `web_search`).
        name: String,
        /// Tool arguments as JSON.
        input: serde_json::Value,
    },
    /// The result of a server-side `web_search` invocation.
    WebSearchToolResult {
        /// The `id` of the [`KnownBlock::ServerToolUse`] this result corresponds to.
        tool_use_id: String,
        /// Result payload (search hits etc.); shape is server-defined.
        content: serde_json::Value,
    },
}

/// `type` tags recognized by this SDK version. Used by [`ContentBlock`]'s
/// `Deserialize` impl to decide between `Known` and `Other`.
const KNOWN_BLOCK_TAGS: &[&str] = &[
    "text",
    "image",
    "document",
    "tool_use",
    "tool_result",
    "thinking",
    "redacted_thinking",
    "server_tool_use",
    "web_search_tool_result",
];

impl Serialize for ContentBlock {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ContentBlock::Known(k) => k.serialize(s),
            ContentBlock::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        let type_tag = value.get("type").and_then(serde_json::Value::as_str);
        match type_tag {
            Some(t) if KNOWN_BLOCK_TAGS.contains(&t) => {
                let known: KnownBlock =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(ContentBlock::Known(known))
            }
            _ => Ok(ContentBlock::Other(value)),
        }
    }
}

impl From<KnownBlock> for ContentBlock {
    fn from(k: KnownBlock) -> Self {
        ContentBlock::Known(k)
    }
}

impl ContentBlock {
    /// If this is a known block, return the inner [`KnownBlock`].
    pub fn known(&self) -> Option<&KnownBlock> {
        match self {
            Self::Known(k) => Some(k),
            Self::Other(_) => None,
        }
    }

    /// If this is an unknown block, return the raw JSON.
    pub fn other(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Other(v) => Some(v),
            Self::Known(_) => None,
        }
    }

    /// Returns the wire-level `type` tag for this block, regardless of variant.
    ///
    /// For known blocks this returns the `snake_case` discriminant; for unknown
    /// blocks it returns whatever string the server sent in the `type` field
    /// (or `None` if the field was missing or non-string).
    pub fn type_tag(&self) -> Option<&str> {
        match self {
            Self::Known(k) => Some(known_type_tag(k)),
            Self::Other(v) => v.get("type").and_then(serde_json::Value::as_str),
        }
    }

    /// Convenience constructor for a plain text block.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Known(KnownBlock::Text {
            text: s.into(),
            cache_control: None,
            citations: None,
        })
    }

    /// Convenience constructor for a URL-sourced image block.
    ///
    /// ```
    /// use claude_api::messages::ContentBlock;
    /// let block = ContentBlock::image_url("https://example.com/cat.png");
    /// assert_eq!(block.type_tag(), Some("image"));
    /// ```
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::Known(KnownBlock::Image {
            source: ImageSource::Url { url: url.into() },
            cache_control: None,
        })
    }

    /// Convenience constructor for a base64-encoded image block. `media_type`
    /// is the IANA MIME type (e.g. `"image/png"`); `data` is base64.
    pub fn image_base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::Known(KnownBlock::Image {
            source: ImageSource::Base64 {
                media_type: media_type.into(),
                data: data.into(),
            },
            cache_control: None,
        })
    }

    /// Convenience constructor for an inline-text document block. Cites the
    /// document by `title` if provided.
    ///
    /// ```
    /// use claude_api::messages::ContentBlock;
    /// let block = ContentBlock::document_text("Page contents.", Some("Spec"));
    /// assert_eq!(block.type_tag(), Some("document"));
    /// ```
    pub fn document_text(data: impl Into<String>, title: Option<&str>) -> Self {
        Self::Known(KnownBlock::Document {
            source: DocumentSource::Text {
                media_type: "text/plain".to_owned(),
                data: data.into(),
            },
            title: title.map(str::to_owned),
            citations: Some(CitationConfig { enabled: true }),
            cache_control: None,
        })
    }

    /// Convenience constructor for a URL-sourced document block.
    pub fn document_url(url: impl Into<String>) -> Self {
        Self::Known(KnownBlock::Document {
            source: DocumentSource::Url { url: url.into() },
            title: None,
            citations: Some(CitationConfig { enabled: true }),
            cache_control: None,
        })
    }

    /// Convenience constructor: a text block with an ephemeral cache
    /// breakpoint at the default (5-minute) TTL. Use this on the last
    /// block of a long-lived prefix you expect to reuse across requests.
    ///
    /// ```
    /// use claude_api::messages::ContentBlock;
    /// let block = ContentBlock::text_cached("Be concise.");
    /// assert_eq!(block.type_tag(), Some("text"));
    /// ```
    pub fn text_cached(text: impl Into<String>) -> Self {
        Self::Known(KnownBlock::Text {
            text: text.into(),
            cache_control: Some(CacheControl::ephemeral()),
            citations: None,
        })
    }
}

fn known_type_tag(k: &KnownBlock) -> &'static str {
    match k {
        KnownBlock::Text { .. } => "text",
        KnownBlock::Image { .. } => "image",
        KnownBlock::Document { .. } => "document",
        KnownBlock::ToolUse { .. } => "tool_use",
        KnownBlock::ToolResult { .. } => "tool_result",
        KnownBlock::Thinking { .. } => "thinking",
        KnownBlock::RedactedThinking { .. } => "redacted_thinking",
        KnownBlock::ServerToolUse { .. } => "server_tool_use",
        KnownBlock::WebSearchToolResult { .. } => "web_search_tool_result",
    }
}

/// Source of bytes for an [`KnownBlock::Image`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ImageSource {
    /// Inline base64-encoded bytes.
    Base64 {
        /// MIME type (e.g. `image/png`).
        media_type: String,
        /// Base64-encoded image bytes.
        data: String,
    },
    /// Public URL the server should fetch.
    Url {
        /// Image URL.
        url: String,
    },
    /// Reference to an uploaded file.
    File {
        /// File ID returned by the Files API.
        file_id: String,
    },
}

/// Source of bytes for an [`KnownBlock::Document`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DocumentSource {
    /// Inline base64-encoded bytes.
    Base64 {
        /// MIME type (e.g. `application/pdf`).
        media_type: String,
        /// Base64-encoded document bytes.
        data: String,
    },
    /// Public URL the server should fetch.
    Url {
        /// Document URL.
        url: String,
    },
    /// Reference to an uploaded file.
    File {
        /// File ID returned by the Files API.
        file_id: String,
    },
    /// Inline plain-text document. The API requires `media_type` for this
    /// variant (typically `"text/plain"`); use [`ContentBlock::document_text`]
    /// for the common-case constructor.
    Text {
        /// MIME type, e.g. `"text/plain"`. Required by the API.
        media_type: String,
        /// Document text.
        data: String,
    },
}

/// Content payload of a `tool_result` block.
///
/// May be a plain string or a list of further [`ContentBlock`]s (e.g. text + image).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// Plain-text result.
    Text(String),
    /// Structured result composed of further content blocks.
    Blocks(Vec<ContentBlock>),
}

/// Per-document citation configuration on a [`KnownBlock::Document`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CitationConfig {
    /// Whether the model should cite this document in its response.
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn round_trip_block(block: &ContentBlock, expected: &serde_json::Value) {
        let serialized = serde_json::to_value(block).expect("serialize");
        assert_eq!(&serialized, expected, "wire form mismatch");
        let parsed: ContentBlock = serde_json::from_value(serialized).expect("deserialize");
        assert_eq!(&parsed, block, "round-trip mismatch");
    }

    #[test]
    fn text_block_round_trips() {
        round_trip_block(
            &ContentBlock::text("hello"),
            &json!({"type": "text", "text": "hello"}),
        );
    }

    #[test]
    fn text_block_with_cache_control_round_trips() {
        let block = ContentBlock::Known(KnownBlock::Text {
            text: "cached".into(),
            cache_control: Some(CacheControl::ephemeral_ttl("1h")),
            citations: None,
        });
        round_trip_block(
            &block,
            &json!({
                "type": "text",
                "text": "cached",
                "cache_control": {"type": "ephemeral", "ttl": "1h"}
            }),
        );
    }

    #[test]
    fn image_block_url_source_round_trips() {
        let block = ContentBlock::Known(KnownBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/cat.png".into(),
            },
            cache_control: None,
        });
        round_trip_block(
            &block,
            &json!({
                "type": "image",
                "source": {"type": "url", "url": "https://example.com/cat.png"}
            }),
        );
    }

    #[test]
    fn document_block_with_text_source_round_trips() {
        let block = ContentBlock::Known(KnownBlock::Document {
            source: DocumentSource::Text {
                media_type: "text/plain".into(),
                data: "page contents".into(),
            },
            title: Some("Spec".into()),
            citations: Some(CitationConfig { enabled: true }),
            cache_control: None,
        });
        round_trip_block(
            &block,
            &json!({
                "type": "document",
                "source": {"type": "text", "media_type": "text/plain", "data": "page contents"},
                "title": "Spec",
                "citations": {"enabled": true}
            }),
        );
    }

    #[test]
    fn tool_use_round_trips() {
        let block = ContentBlock::Known(KnownBlock::ToolUse {
            id: "toolu_01".into(),
            name: "get_weather".into(),
            input: json!({"city": "Paris"}),
        });
        round_trip_block(
            &block,
            &json!({
                "type": "tool_use",
                "id": "toolu_01",
                "name": "get_weather",
                "input": {"city": "Paris"}
            }),
        );
    }

    #[test]
    fn tool_result_with_string_content_round_trips() {
        let block = ContentBlock::Known(KnownBlock::ToolResult {
            tool_use_id: "toolu_01".into(),
            content: ToolResultContent::Text("72F".into()),
            is_error: None,
            cache_control: None,
        });
        round_trip_block(
            &block,
            &json!({
                "type": "tool_result",
                "tool_use_id": "toolu_01",
                "content": "72F"
            }),
        );
    }

    #[test]
    fn tool_result_with_nested_blocks_round_trips() {
        let block = ContentBlock::Known(KnownBlock::ToolResult {
            tool_use_id: "toolu_01".into(),
            content: ToolResultContent::Blocks(vec![ContentBlock::text("see below")]),
            is_error: Some(false),
            cache_control: None,
        });
        round_trip_block(
            &block,
            &json!({
                "type": "tool_result",
                "tool_use_id": "toolu_01",
                "content": [{"type": "text", "text": "see below"}],
                "is_error": false
            }),
        );
    }

    #[test]
    fn thinking_block_round_trips() {
        let block = ContentBlock::Known(KnownBlock::Thinking {
            thinking: "let me think...".into(),
            signature: "sig".into(),
        });
        round_trip_block(
            &block,
            &json!({
                "type": "thinking",
                "thinking": "let me think...",
                "signature": "sig"
            }),
        );
    }

    #[test]
    fn redacted_thinking_block_round_trips() {
        let block = ContentBlock::Known(KnownBlock::RedactedThinking {
            data: "<opaque>".into(),
        });
        round_trip_block(
            &block,
            &json!({"type": "redacted_thinking", "data": "<opaque>"}),
        );
    }

    #[test]
    fn server_tool_use_round_trips() {
        let block = ContentBlock::Known(KnownBlock::ServerToolUse {
            id: "stu_01".into(),
            name: "web_search".into(),
            input: json!({"query": "rust"}),
        });
        round_trip_block(
            &block,
            &json!({
                "type": "server_tool_use",
                "id": "stu_01",
                "name": "web_search",
                "input": {"query": "rust"}
            }),
        );
    }

    #[test]
    fn web_search_tool_result_round_trips() {
        let block = ContentBlock::Known(KnownBlock::WebSearchToolResult {
            tool_use_id: "stu_01".into(),
            content: json!([{"url": "https://rust-lang.org"}]),
        });
        round_trip_block(
            &block,
            &json!({
                "type": "web_search_tool_result",
                "tool_use_id": "stu_01",
                "content": [{"url": "https://rust-lang.org"}]
            }),
        );
    }

    #[test]
    fn unknown_block_type_falls_back_to_other_preserving_json() {
        let raw = json!({
            "type": "future_block_type",
            "some_field": 42,
            "nested": {"a": "b"}
        });
        let block: ContentBlock = serde_json::from_value(raw.clone()).expect("deserialize");
        match &block {
            ContentBlock::Other(v) => assert_eq!(v, &raw),
            ContentBlock::Known(_) => panic!("expected Other, got Known"),
        }
        let reserialized = serde_json::to_value(&block).expect("serialize");
        assert_eq!(reserialized, raw, "Other must round-trip byte-for-byte");
    }

    #[test]
    fn missing_type_field_falls_back_to_other() {
        let raw = json!({"text": "hi"});
        let block: ContentBlock = serde_json::from_value(raw.clone()).expect("deserialize");
        match &block {
            ContentBlock::Other(v) => assert_eq!(v, &raw),
            ContentBlock::Known(_) => panic!("expected Other"),
        }
    }

    #[test]
    fn malformed_known_block_is_an_error_not_other() {
        // Known type tag but `text` field is the wrong shape.
        let raw = json!({"type": "text", "text": 42});
        let result: Result<ContentBlock, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "malformed known type must error, not silently fall through to Other"
        );
    }

    #[test]
    fn type_tag_works_for_known_and_other() {
        assert_eq!(ContentBlock::text("x").type_tag(), Some("text"));

        let other_json = json!({"type": "future_thing", "x": 1});
        let other: ContentBlock = serde_json::from_value(other_json).unwrap();
        assert_eq!(other.type_tag(), Some("future_thing"));
    }

    #[test]
    fn known_and_other_accessors() {
        let known = ContentBlock::text("hi");
        assert!(known.known().is_some());
        assert!(known.other().is_none());

        let other: ContentBlock =
            serde_json::from_value(json!({"type": "future", "x": 1})).unwrap();
        assert!(other.known().is_none());
        assert!(other.other().is_some());
    }

    #[test]
    fn from_known_block_into_content_block() {
        let kb = KnownBlock::Text {
            text: "via from".into(),
            cache_control: None,
            citations: None,
        };
        let cb: ContentBlock = kb.into();
        assert_eq!(cb.type_tag(), Some("text"));
    }
}
