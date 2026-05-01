//! Streaming event types and reconstruction.
//!
//! Both [`StreamEvent`] and [`ContentDelta`] are forward-compatible: unknown
//! `type` tags deserialize into the `Other` arm with the raw JSON preserved
//! byte-for-byte. Strict-on-known semantics: a known tag with a malformed
//! body returns a deserialization error rather than silently falling through.
//!
//! [`EventStream`] is the typed wrapper around the SSE wire format; call
//! [`EventStream::aggregate`] to reconstruct a [`Message`] from a full
//! `message_start → ... → message_stop` sequence.

use serde::{Deserialize, Serialize};

use crate::error::ApiErrorPayload;
use crate::forward_compat::dispatch_known_or_other;
use crate::messages::content::ContentBlock;
use crate::messages::response::Message;
use crate::types::{StopReason, Usage};

#[cfg(feature = "streaming")]
use crate::error::{Error, Result, StreamError};
#[cfg(feature = "streaming")]
use crate::messages::content::KnownBlock;

/// A single event from the Messages streaming endpoint.
///
/// Forward-compatible wrapper around [`KnownStreamEvent`]; unknown event types
/// land in [`StreamEvent::Other`] preserving the raw JSON.
//
// Suppress `large_enum_variant`: boxing Known would break pattern-match
// ergonomics. Worth revisiting in a v1.0 release that's free to break the
// stream-event API.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// An event whose `type` is recognized by this SDK version.
    Known(KnownStreamEvent),
    /// An event whose `type` is not recognized; the raw JSON is preserved.
    Other(serde_json::Value),
}

/// All streaming event types known to this SDK version.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnownStreamEvent {
    /// Begins a new streamed message; carries the empty [`Message`] shell
    /// that subsequent events will fill in.
    MessageStart {
        /// The opening message snapshot.
        message: Message,
    },
    /// Begins a new content block within the message.
    ContentBlockStart {
        /// Index of the block within the message's content array.
        index: u32,
        /// Initial state of the block.
        content_block: ContentBlock,
    },
    /// Incremental update to a content block.
    ContentBlockDelta {
        /// Index of the block being updated.
        index: u32,
        /// The delta payload.
        delta: ContentDelta,
    },
    /// Marks a content block as complete.
    ContentBlockStop {
        /// Index of the block that finished.
        index: u32,
    },
    /// Late-arriving updates to message-level fields, plus final usage.
    MessageDelta {
        /// Updated message-level fields.
        delta: MessageDelta,
        /// Cumulative usage at the point this delta was emitted.
        usage: Usage,
    },
    /// Final event in a successful stream.
    MessageStop,
    /// Keep-alive ping; no payload.
    Ping,
    /// Server reported a fatal error mid-stream.
    Error {
        /// The error payload.
        error: ApiErrorPayload,
    },
}

/// `type` tags this SDK recognizes for streaming events.
const KNOWN_EVENT_TAGS: &[&str] = &[
    "message_start",
    "content_block_start",
    "content_block_delta",
    "content_block_stop",
    "message_delta",
    "message_stop",
    "ping",
    "error",
];

impl Serialize for StreamEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            StreamEvent::Known(k) => k.serialize(s),
            StreamEvent::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for StreamEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        dispatch_known_or_other(
            raw,
            KNOWN_EVENT_TAGS,
            StreamEvent::Known,
            StreamEvent::Other,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl From<KnownStreamEvent> for StreamEvent {
    fn from(k: KnownStreamEvent) -> Self {
        StreamEvent::Known(k)
    }
}

impl StreamEvent {
    /// If this is a known event, return the inner [`KnownStreamEvent`].
    pub fn known(&self) -> Option<&KnownStreamEvent> {
        match self {
            Self::Known(k) => Some(k),
            Self::Other(_) => None,
        }
    }

    /// If this is an unknown event, return the raw JSON.
    pub fn other(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Other(v) => Some(v),
            Self::Known(_) => None,
        }
    }

    /// Wire-level `type` tag for this event regardless of variant.
    pub fn type_tag(&self) -> Option<&str> {
        match self {
            Self::Known(k) => Some(known_event_tag(k)),
            Self::Other(v) => v.get("type").and_then(serde_json::Value::as_str),
        }
    }
}

fn known_event_tag(k: &KnownStreamEvent) -> &'static str {
    match k {
        KnownStreamEvent::MessageStart { .. } => "message_start",
        KnownStreamEvent::ContentBlockStart { .. } => "content_block_start",
        KnownStreamEvent::ContentBlockDelta { .. } => "content_block_delta",
        KnownStreamEvent::ContentBlockStop { .. } => "content_block_stop",
        KnownStreamEvent::MessageDelta { .. } => "message_delta",
        KnownStreamEvent::MessageStop => "message_stop",
        KnownStreamEvent::Ping => "ping",
        KnownStreamEvent::Error { .. } => "error",
    }
}

/// Late-arriving updates to message-level fields, emitted in
/// [`KnownStreamEvent::MessageDelta`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessageDelta {
    /// Why the model stopped (if known at this point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// Stop sequence that triggered termination, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// One delta update inside a [`KnownStreamEvent::ContentBlockDelta`].
///
/// Forward-compatible wrapper around [`KnownContentDelta`].
#[derive(Debug, Clone, PartialEq)]
pub enum ContentDelta {
    /// A delta whose `type` is recognized by this SDK version.
    Known(KnownContentDelta),
    /// A delta whose `type` is not recognized; the raw JSON is preserved.
    Other(serde_json::Value),
}

/// All content-delta variants known to this SDK version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnownContentDelta {
    /// Append text to a `text` block.
    TextDelta {
        /// Additional text.
        text: String,
    },
    /// Append a partial-JSON fragment to a `tool_use`'s `input`.
    InputJsonDelta {
        /// Partial JSON fragment.
        partial_json: String,
    },
    /// Append text to a `thinking` block.
    ThinkingDelta {
        /// Additional thinking text.
        thinking: String,
    },
    /// Update the `signature` of a `thinking` block.
    SignatureDelta {
        /// Updated signature.
        signature: String,
    },
    /// Append a citation to a `text` block.
    CitationsDelta {
        /// The citation payload (typed enum with forward-compat fallback).
        citation: crate::messages::citation::Citation,
    },
}

const KNOWN_DELTA_TAGS: &[&str] = &[
    "text_delta",
    "input_json_delta",
    "thinking_delta",
    "signature_delta",
    "citations_delta",
];

impl Serialize for ContentDelta {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ContentDelta::Known(k) => k.serialize(s),
            ContentDelta::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ContentDelta {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        dispatch_known_or_other(
            raw,
            KNOWN_DELTA_TAGS,
            ContentDelta::Known,
            ContentDelta::Other,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl From<KnownContentDelta> for ContentDelta {
    fn from(k: KnownContentDelta) -> Self {
        ContentDelta::Known(k)
    }
}

impl ContentDelta {
    /// If this is a known delta, return the inner [`KnownContentDelta`].
    pub fn known(&self) -> Option<&KnownContentDelta> {
        match self {
            Self::Known(k) => Some(k),
            Self::Other(_) => None,
        }
    }

    /// If this is an unknown delta, return the raw JSON.
    pub fn other(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Other(v) => Some(v),
            Self::Known(_) => None,
        }
    }

    /// Wire-level `type` tag for this delta regardless of variant.
    pub fn type_tag(&self) -> Option<&str> {
        match self {
            Self::Known(k) => Some(match k {
                KnownContentDelta::TextDelta { .. } => "text_delta",
                KnownContentDelta::InputJsonDelta { .. } => "input_json_delta",
                KnownContentDelta::ThinkingDelta { .. } => "thinking_delta",
                KnownContentDelta::SignatureDelta { .. } => "signature_delta",
                KnownContentDelta::CitationsDelta { .. } => "citations_delta",
            }),
            Self::Other(v) => v.get("type").and_then(serde_json::Value::as_str),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ApiErrorKind;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn round_trip_event(event: &StreamEvent, expected: &serde_json::Value) {
        let v = serde_json::to_value(event).expect("serialize");
        assert_eq!(&v, expected, "wire form mismatch");
        let parsed: StreamEvent = serde_json::from_value(v).expect("deserialize");
        assert_eq!(&parsed, event, "round-trip mismatch");
    }

    fn round_trip_delta(delta: &ContentDelta, expected: &serde_json::Value) {
        let v = serde_json::to_value(delta).expect("serialize");
        assert_eq!(&v, expected, "wire form mismatch");
        let parsed: ContentDelta = serde_json::from_value(v).expect("deserialize");
        assert_eq!(&parsed, delta, "round-trip mismatch");
    }

    // ---- StreamEvent variants ----

    #[test]
    fn message_stop_round_trips() {
        round_trip_event(
            &StreamEvent::Known(KnownStreamEvent::MessageStop),
            &json!({"type": "message_stop"}),
        );
    }

    #[test]
    fn ping_round_trips() {
        round_trip_event(
            &StreamEvent::Known(KnownStreamEvent::Ping),
            &json!({"type": "ping"}),
        );
    }

    #[test]
    fn content_block_start_round_trips() {
        let ev = StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::text(""),
        });
        round_trip_event(
            &ev,
            &json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
        );
    }

    #[test]
    fn content_block_delta_round_trips() {
        let ev = StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::Known(KnownContentDelta::TextDelta {
                text: "Hello".into(),
            }),
        });
        round_trip_event(
            &ev,
            &json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {"type": "text_delta", "text": "Hello"}
            }),
        );
    }

    #[test]
    fn content_block_stop_round_trips() {
        let ev = StreamEvent::Known(KnownStreamEvent::ContentBlockStop { index: 2 });
        round_trip_event(&ev, &json!({"type": "content_block_stop", "index": 2}));
    }

    #[test]
    fn message_delta_round_trips() {
        let ev = StreamEvent::Known(KnownStreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some(StopReason::EndTurn),
                stop_sequence: None,
            },
            usage: Usage {
                input_tokens: 5,
                output_tokens: 10,
                ..Usage::default()
            },
        });
        round_trip_event(
            &ev,
            &json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"input_tokens": 5, "output_tokens": 10}
            }),
        );
    }

    #[test]
    fn error_event_round_trips() {
        let ev = StreamEvent::Known(KnownStreamEvent::Error {
            error: ApiErrorPayload {
                kind: ApiErrorKind::OverloadedError,
                message: "try again".into(),
            },
        });
        round_trip_event(
            &ev,
            &json!({
                "type": "error",
                "error": {"type": "overloaded_error", "message": "try again"}
            }),
        );
    }

    // ---- Forward-compat ----

    #[test]
    fn unknown_event_type_falls_back_to_other_preserving_json() {
        let raw = json!({
            "type": "future_event",
            "payload": {"x": 1, "y": [2, 3]}
        });
        let ev: StreamEvent = serde_json::from_value(raw.clone()).expect("deserialize");
        assert!(ev.other().is_some());
        assert_eq!(ev.type_tag(), Some("future_event"));

        let reserialized = serde_json::to_value(&ev).expect("serialize");
        assert_eq!(reserialized, raw, "Other must round-trip byte-for-byte");
    }

    #[test]
    fn malformed_known_event_is_an_error() {
        // Known type, but `index` should be a u32, not a string.
        let raw = json!({"type": "content_block_stop", "index": "nope"});
        let result: Result<StreamEvent, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "malformed known event must error, not silently fall through to Other"
        );
    }

    // ---- ContentDelta variants ----

    #[test]
    fn text_delta_round_trips() {
        round_trip_delta(
            &ContentDelta::Known(KnownContentDelta::TextDelta { text: "hi".into() }),
            &json!({"type": "text_delta", "text": "hi"}),
        );
    }

    #[test]
    fn input_json_delta_round_trips() {
        round_trip_delta(
            &ContentDelta::Known(KnownContentDelta::InputJsonDelta {
                partial_json: r#"{"city":"P"#.into(),
            }),
            &json!({"type": "input_json_delta", "partial_json": "{\"city\":\"P"}),
        );
    }

    #[test]
    fn thinking_delta_round_trips() {
        round_trip_delta(
            &ContentDelta::Known(KnownContentDelta::ThinkingDelta {
                thinking: " more thinking".into(),
            }),
            &json!({"type": "thinking_delta", "thinking": " more thinking"}),
        );
    }

    #[test]
    fn signature_delta_round_trips() {
        round_trip_delta(
            &ContentDelta::Known(KnownContentDelta::SignatureDelta {
                signature: "sig123".into(),
            }),
            &json!({"type": "signature_delta", "signature": "sig123"}),
        );
    }

    #[test]
    fn citations_delta_round_trips() {
        use crate::messages::citation::{Citation, KnownCitation};
        round_trip_delta(
            &ContentDelta::Known(KnownContentDelta::CitationsDelta {
                citation: Citation::Known(KnownCitation::CharLocation {
                    document_index: 0,
                    document_title: Some("Doc".into()),
                    cited_text: "hello".into(),
                    start_char_index: 0,
                    end_char_index: 5,
                }),
            }),
            &json!({
                "type": "citations_delta",
                "citation": {
                    "type": "char_location",
                    "document_index": 0,
                    "document_title": "Doc",
                    "cited_text": "hello",
                    "start_char_index": 0,
                    "end_char_index": 5
                }
            }),
        );
    }

    #[test]
    fn unknown_delta_type_falls_back_to_other_preserving_json() {
        let raw = json!({"type": "future_delta", "stuff": [1, 2]});
        let d: ContentDelta = serde_json::from_value(raw.clone()).expect("deserialize");
        assert!(d.other().is_some());
        assert_eq!(d.type_tag(), Some("future_delta"));
        let reserialized = serde_json::to_value(&d).expect("serialize");
        assert_eq!(reserialized, raw);
    }

    #[test]
    fn malformed_known_delta_is_an_error() {
        let raw = json!({"type": "text_delta", "text": 42});
        let result: Result<ContentDelta, _> = serde_json::from_value(raw);
        assert!(result.is_err());
    }

    // ---- Golden sequence: a typical stream from start to stop ----

    #[test]
    fn golden_sequence_decodes_end_to_end() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_X",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-sonnet-4-6",
                    "usage": {"input_tokens": 10, "output_tokens": 0}
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": " world"}
            }),
            json!({"type": "content_block_stop", "index": 0}),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"input_tokens": 10, "output_tokens": 2}
            }),
            json!({"type": "message_stop"}),
        ];

        let parsed: Vec<StreamEvent> = events
            .into_iter()
            .map(|v| serde_json::from_value(v).expect("decode"))
            .collect();

        assert_eq!(parsed.len(), 7);
        assert_eq!(parsed[0].type_tag(), Some("message_start"));
        assert_eq!(parsed[6].type_tag(), Some("message_stop"));

        // The two text_delta events should match.
        match &parsed[2] {
            StreamEvent::Known(KnownStreamEvent::ContentBlockDelta { delta, .. }) => match delta {
                ContentDelta::Known(KnownContentDelta::TextDelta { text }) => {
                    assert_eq!(text, "Hello");
                }
                _ => panic!("expected TextDelta"),
            },
            _ => panic!("expected ContentBlockDelta"),
        }
    }
}

// ---------------------------------------------------------------------------
// EventStream + Aggregator (gated on the `streaming` feature)
// ---------------------------------------------------------------------------

/// Typed stream of [`StreamEvent`]s yielded from a streaming Messages request.
///
/// Implements [`futures_util::Stream`] so callers can iterate event-by-event,
/// or call [`Self::aggregate`] to drive the stream to completion and
/// reconstruct a full [`Message`].
///
/// Optional **callback hooks** can be attached via the `on_*` builder
/// methods; they fire only during [`Self::aggregate`] (the raw `Stream`
/// path is unaffected). Useful for token-by-token UI updates without
/// pattern-matching `StreamEvent` yourself.
///
/// Mid-stream connection failures are not retried -- doing so would silently
/// drop content. See [`crate::error::Error::is_retryable`].
#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
pub struct EventStream {
    inner: futures_util::stream::BoxStream<'static, Result<StreamEvent>>,
    handlers: MessageStreamHandlers,
}

#[cfg(feature = "streaming")]
impl EventStream {
    /// Wrap a streaming HTTP response.
    pub(crate) fn from_response(response: reqwest::Response) -> Self {
        use futures_util::StreamExt;
        Self {
            inner: crate::sse::into_typed_stream::<StreamEvent>(response).boxed(),
            handlers: MessageStreamHandlers::default(),
        }
    }

    /// Test helper: build an `EventStream` from a pre-baked sequence of
    /// `Result<StreamEvent>`s. Used to exercise callback wiring without a
    /// real HTTP connection.
    #[cfg(test)]
    fn from_events(events: Vec<Result<StreamEvent>>) -> Self {
        use futures_util::StreamExt;
        Self {
            inner: futures_util::stream::iter(events).boxed(),
            handlers: MessageStreamHandlers::default(),
        }
    }

    /// Attach a handler fired on each text-delta inside a `text` content block.
    /// The closure receives only the new chunk; the running concatenation is
    /// available via the final [`Message`] returned by [`Self::aggregate`].
    #[must_use]
    pub fn on_text_delta<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.handlers.text_delta = Some(Box::new(handler));
        self
    }

    /// Attach a handler fired when a `tool_use` content block finishes
    /// streaming (its `input` JSON is fully reconstructed). The closure
    /// receives `(id, name, &input)`. Also fires for `server_tool_use`
    /// blocks (e.g. web search invocations).
    #[must_use]
    pub fn on_tool_use_complete<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&str, &str, &serde_json::Value) + Send + 'static,
    {
        self.handlers.tool_use_complete = Some(Box::new(handler));
        self
    }

    /// Attach a handler fired on each delta inside a `thinking` block.
    #[must_use]
    pub fn on_thinking_delta<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.handlers.thinking_delta = Some(Box::new(handler));
        self
    }

    /// Attach a handler fired once when the stream's final `message_stop`
    /// event arrives. Receives the cumulative [`Usage`] from the message.
    #[must_use]
    pub fn on_message_stop<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&Usage) + Send + 'static,
    {
        self.handlers.message_stop = Some(Box::new(handler));
        self
    }

    /// Attach a handler fired when the server emits an `error` stream event
    /// or when a stream-parse failure escapes mid-aggregation. The closure
    /// runs before the error propagates back to the caller of
    /// [`Self::aggregate`].
    #[must_use]
    pub fn on_error<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&Error) + Send + 'static,
    {
        self.handlers.error = Some(Box::new(handler));
        self
    }

    /// Drive the stream to completion and return the reconstructed [`Message`].
    ///
    /// Equivalent to using `messages.create(...)` non-streamed -- the same
    /// final [`Message`] payload is produced. If callback hooks were
    /// attached via the `on_*` builder methods, they fire as their
    /// corresponding events are processed.
    pub async fn aggregate(self) -> Result<Message> {
        use futures_util::StreamExt;
        let Self {
            mut inner,
            handlers,
        } = self;
        let mut agg = Aggregator::with_handlers(handlers);
        while let Some(event) = inner.next().await {
            match event {
                Ok(ev) => match agg.handle(ev) {
                    Ok(()) => {}
                    Err(e) => {
                        agg.fire_error(&e);
                        return Err(e);
                    }
                },
                Err(e) => {
                    agg.fire_error(&e);
                    return Err(e);
                }
            }
        }
        agg.finalize()
    }
}

#[cfg(feature = "streaming")]
type TextDeltaHandler = Box<dyn FnMut(&str) + Send>;
#[cfg(feature = "streaming")]
type ToolUseCompleteHandler = Box<dyn FnMut(&str, &str, &serde_json::Value) + Send>;
#[cfg(feature = "streaming")]
type ThinkingDeltaHandler = Box<dyn FnMut(&str) + Send>;
#[cfg(feature = "streaming")]
type MessageStopHandler = Box<dyn FnMut(&Usage) + Send>;
#[cfg(feature = "streaming")]
type ErrorHandler = Box<dyn FnMut(&Error) + Send>;

/// Callback hooks fired during [`EventStream::aggregate`].
#[cfg(feature = "streaming")]
#[derive(Default)]
struct MessageStreamHandlers {
    text_delta: Option<TextDeltaHandler>,
    tool_use_complete: Option<ToolUseCompleteHandler>,
    thinking_delta: Option<ThinkingDeltaHandler>,
    message_stop: Option<MessageStopHandler>,
    error: Option<ErrorHandler>,
}

#[cfg(feature = "streaming")]
impl std::fmt::Debug for MessageStreamHandlers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageStreamHandlers")
            .field("text_delta", &self.text_delta.as_ref().map(|_| "<fn>"))
            .field(
                "tool_use_complete",
                &self.tool_use_complete.as_ref().map(|_| "<fn>"),
            )
            .field(
                "thinking_delta",
                &self.thinking_delta.as_ref().map(|_| "<fn>"),
            )
            .field("message_stop", &self.message_stop.as_ref().map(|_| "<fn>"))
            .field("error", &self.error.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

#[cfg(feature = "streaming")]
impl futures_util::Stream for EventStream {
    type Item = Result<StreamEvent>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

#[cfg(feature = "streaming")]
impl std::fmt::Debug for EventStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}

/// Reconstructs a [`Message`] from a sequence of [`StreamEvent`]s.
///
/// Pure data structure -- no I/O. Designed to be testable in isolation by
/// feeding events directly via [`Self::handle`].
#[cfg(feature = "streaming")]
#[derive(Debug, Default)]
pub struct Aggregator {
    message: Option<Message>,
    blocks: Vec<ContentBlock>,
    /// Accumulated `partial_json` strings per block index, parsed at
    /// `ContentBlockStop` and stored back on the corresponding `ToolUse`
    /// or `ServerToolUse` block's `input`.
    tool_input_buffers: std::collections::HashMap<u32, String>,
    handlers: MessageStreamHandlers,
}

#[cfg(feature = "streaming")]
impl Aggregator {
    /// Build an Aggregator pre-populated with stream callback hooks.
    /// Internal -- callers wire handlers through [`EventStream`].
    fn with_handlers(handlers: MessageStreamHandlers) -> Self {
        Self {
            handlers,
            ..Self::default()
        }
    }

    /// Fire the `error` handler for an aborting error. Internal helper
    /// for [`EventStream::aggregate`].
    fn fire_error(&mut self, err: &Error) {
        if let Some(h) = self.handlers.error.as_mut() {
            h(err);
        }
    }

    /// Apply one event to the aggregator's state.
    pub fn handle(&mut self, event: StreamEvent) -> Result<()> {
        match event {
            StreamEvent::Known(known) => self.handle_known(known),
            StreamEvent::Other(value) => {
                tracing::debug!(?value, "claude-api: ignoring unknown stream event");
                Ok(())
            }
        }
    }

    fn handle_known(&mut self, event: KnownStreamEvent) -> Result<()> {
        match event {
            KnownStreamEvent::MessageStart { message } => {
                self.message = Some(message);
            }
            KnownStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                if index as usize != self.blocks.len() {
                    return Err(Error::Stream(StreamError::Parse(format!(
                        "out-of-order content_block_start: index {} but {} blocks already received",
                        index,
                        self.blocks.len()
                    ))));
                }
                self.blocks.push(content_block);
            }
            KnownStreamEvent::ContentBlockDelta { index, delta } => {
                self.apply_delta(index, delta);
            }
            KnownStreamEvent::ContentBlockStop { index } => {
                if let Some(buf) = self.tool_input_buffers.remove(&index) {
                    self.finalize_tool_input(index, &buf);
                }
                // Fire on_tool_use_complete for tool_use / server_tool_use blocks.
                if let Some(handler) = self.handlers.tool_use_complete.as_mut() {
                    if let Some(ContentBlock::Known(
                        KnownBlock::ToolUse { id, name, input }
                        | KnownBlock::ServerToolUse { id, name, input },
                    )) = self.blocks.get(index as usize)
                    {
                        handler(id, name, input);
                    }
                }
            }
            KnownStreamEvent::MessageDelta { delta, usage } => {
                if let Some(msg) = self.message.as_mut() {
                    if let Some(sr) = delta.stop_reason {
                        msg.stop_reason = Some(sr);
                    }
                    if let Some(ss) = delta.stop_sequence {
                        msg.stop_sequence = Some(ss);
                    }
                    msg.usage = usage;
                }
            }
            KnownStreamEvent::MessageStop => {
                if let Some(handler) = self.handlers.message_stop.as_mut() {
                    if let Some(msg) = self.message.as_ref() {
                        handler(&msg.usage);
                    }
                }
            }
            KnownStreamEvent::Ping => {}
            KnownStreamEvent::Error { error } => {
                return Err(Error::Stream(StreamError::Server {
                    kind: error.kind,
                    message: error.message,
                }));
            }
        }
        Ok(())
    }

    fn apply_delta(&mut self, index: u32, delta: ContentDelta) {
        let Some(block) = self.blocks.get_mut(index as usize) else {
            tracing::warn!(index, "claude-api: delta for unknown block index, dropping");
            return;
        };
        match delta {
            ContentDelta::Known(KnownContentDelta::TextDelta { text }) => {
                if let ContentBlock::Known(KnownBlock::Text { text: existing, .. }) = block {
                    existing.push_str(&text);
                }
                if let Some(handler) = self.handlers.text_delta.as_mut() {
                    handler(&text);
                }
            }
            ContentDelta::Known(KnownContentDelta::InputJsonDelta { partial_json }) => {
                self.tool_input_buffers
                    .entry(index)
                    .or_default()
                    .push_str(&partial_json);
            }
            ContentDelta::Known(KnownContentDelta::ThinkingDelta { thinking }) => {
                if let ContentBlock::Known(KnownBlock::Thinking {
                    thinking: existing, ..
                }) = block
                {
                    existing.push_str(&thinking);
                }
                if let Some(handler) = self.handlers.thinking_delta.as_mut() {
                    handler(&thinking);
                }
            }
            ContentDelta::Known(KnownContentDelta::SignatureDelta { signature }) => {
                if let ContentBlock::Known(KnownBlock::Thinking { signature: sig, .. }) = block {
                    *sig = signature;
                }
            }
            ContentDelta::Known(KnownContentDelta::CitationsDelta { citation }) => {
                if let ContentBlock::Known(KnownBlock::Text { citations, .. }) = block {
                    citations.get_or_insert_with(Vec::new).push(citation);
                }
            }
            ContentDelta::Other(value) => {
                tracing::debug!(?value, "claude-api: ignoring unknown content delta");
            }
        }
    }

    fn finalize_tool_input(&mut self, index: u32, buffer: &str) {
        let Some(block) = self.blocks.get_mut(index as usize) else {
            return;
        };
        let parsed = if buffer.is_empty() {
            // Nothing to parse; leave whatever the start event provided.
            return;
        } else {
            serde_json::from_str::<serde_json::Value>(buffer).unwrap_or_else(|e| {
                tracing::warn!(
                    error = %e,
                    "claude-api: tool_use input failed to parse; storing raw string"
                );
                serde_json::Value::String(buffer.to_owned())
            })
        };
        match block {
            ContentBlock::Known(
                KnownBlock::ToolUse { input, .. } | KnownBlock::ServerToolUse { input, .. },
            ) => {
                *input = parsed;
            }
            _ => {
                tracing::warn!(
                    index,
                    "claude-api: input_json_delta accumulated for non-tool-use block"
                );
            }
        }
    }

    /// Finalize: combine the accumulated `MessageStart` shell with the
    /// reconstructed content blocks.
    pub fn finalize(mut self) -> Result<Message> {
        let mut message = self.message.take().ok_or_else(|| {
            Error::Stream(StreamError::Parse(
                "stream ended without a message_start event".into(),
            ))
        })?;
        message.content = self.blocks;
        Ok(message)
    }
}

#[cfg(all(test, feature = "streaming"))]
mod aggregator_tests {
    use super::*;
    use crate::error::{ApiErrorKind, ApiErrorPayload};
    use crate::types::{ModelId, Role};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn message_start_event() -> StreamEvent {
        StreamEvent::Known(KnownStreamEvent::MessageStart {
            message: serde_json::from_value(json!({
                "id": "msg_x",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-6",
                "usage": {"input_tokens": 5, "output_tokens": 0}
            }))
            .unwrap(),
        })
    }

    #[test]
    fn aggregator_reconstructs_text_message() {
        let mut agg = Aggregator::default();
        agg.handle(message_start_event()).unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::text(""),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Known(KnownContentDelta::TextDelta {
                text: "Hello".into(),
            }),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Known(KnownContentDelta::TextDelta {
                text: " world".into(),
            }),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
            index: 0,
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some(StopReason::EndTurn),
                stop_sequence: None,
            },
            usage: Usage {
                input_tokens: 5,
                output_tokens: 2,
                ..Usage::default()
            },
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::MessageStop))
            .unwrap();

        let msg = agg.finalize().unwrap();
        assert_eq!(msg.id, "msg_x");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.model, ModelId::SONNET_4_6);
        assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(msg.usage.output_tokens, 2);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Known(KnownBlock::Text { text, .. }) => {
                assert_eq!(text, "Hello world");
            }
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn aggregator_reconstructs_tool_use_input_from_partial_json_deltas() {
        let mut agg = Aggregator::default();
        agg.handle(message_start_event()).unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Known(KnownBlock::ToolUse {
                id: "toolu_1".into(),
                name: "get_weather".into(),
                input: json!({}),
            }),
        }))
        .unwrap();
        for chunk in ["{\"city\":", "\"Paris\"", ",\"unit\":\"C\"}"] {
            agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::InputJsonDelta {
                    partial_json: chunk.into(),
                }),
            }))
            .unwrap();
        }
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
            index: 0,
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::MessageStop))
            .unwrap();

        let msg = agg.finalize().unwrap();
        match &msg.content[0] {
            ContentBlock::Known(KnownBlock::ToolUse { input, name, .. }) => {
                assert_eq!(name, "get_weather");
                assert_eq!(input, &json!({"city": "Paris", "unit": "C"}));
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn aggregator_reconstructs_thinking_block() {
        let mut agg = Aggregator::default();
        agg.handle(message_start_event()).unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Known(KnownBlock::Thinking {
                thinking: String::new(),
                signature: String::new(),
            }),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Known(KnownContentDelta::ThinkingDelta {
                thinking: "let me ".into(),
            }),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Known(KnownContentDelta::ThinkingDelta {
                thinking: "think".into(),
            }),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Known(KnownContentDelta::SignatureDelta {
                signature: "sig_xyz".into(),
            }),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
            index: 0,
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::MessageStop))
            .unwrap();

        let msg = agg.finalize().unwrap();
        match &msg.content[0] {
            ContentBlock::Known(KnownBlock::Thinking {
                thinking,
                signature,
            }) => {
                assert_eq!(thinking, "let me think");
                assert_eq!(signature, "sig_xyz");
            }
            _ => panic!("expected Thinking block"),
        }
    }

    #[test]
    fn aggregator_unknown_event_is_ignored() {
        let mut agg = Aggregator::default();
        agg.handle(message_start_event()).unwrap();
        // Unknown event should not error.
        agg.handle(StreamEvent::Other(json!({"type": "future_event"})))
            .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::MessageStop))
            .unwrap();
        let msg = agg.finalize().unwrap();
        assert!(msg.content.is_empty());
    }

    #[test]
    fn aggregator_unknown_delta_is_ignored() {
        let mut agg = Aggregator::default();
        agg.handle(message_start_event()).unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::text(""),
        }))
        .unwrap();
        agg.handle(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Other(json!({"type": "future_delta"})),
        }))
        .unwrap();
        // Aggregator should not have crashed.
    }

    #[test]
    fn aggregator_server_error_event_propagates() {
        let mut agg = Aggregator::default();
        agg.handle(message_start_event()).unwrap();
        let err = agg
            .handle(StreamEvent::Known(KnownStreamEvent::Error {
                error: ApiErrorPayload {
                    kind: ApiErrorKind::OverloadedError,
                    message: "boom".into(),
                },
            }))
            .unwrap_err();
        match err {
            Error::Stream(StreamError::Server { kind, message }) => {
                assert_eq!(kind, ApiErrorKind::OverloadedError);
                assert_eq!(message, "boom");
            }
            other => panic!("expected Stream::Server, got {other:?}"),
        }
    }

    #[test]
    fn aggregator_out_of_order_block_start_errors() {
        let mut agg = Aggregator::default();
        agg.handle(message_start_event()).unwrap();
        // Skip index 0; start with index 1.
        let err = agg
            .handle(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlock::text(""),
            }))
            .unwrap_err();
        assert!(matches!(err, Error::Stream(StreamError::Parse(_))));
    }

    #[test]
    fn aggregator_finalize_without_message_start_errors() {
        let agg = Aggregator::default();
        let err = agg.finalize().unwrap_err();
        assert!(matches!(err, Error::Stream(StreamError::Parse(_))));
    }
}

#[cfg(all(test, feature = "streaming"))]
mod stream_callback_tests {
    use super::*;
    use crate::error::{ApiErrorKind, ApiErrorPayload};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn message_start_event() -> StreamEvent {
        StreamEvent::Known(KnownStreamEvent::MessageStart {
            message: serde_json::from_value(json!({
                "id": "msg_x",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-6",
                "usage": {"input_tokens": 5, "output_tokens": 0}
            }))
            .unwrap(),
        })
    }

    #[tokio::test]
    async fn on_text_delta_fires_for_each_text_chunk() {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let events = vec![
            Ok(message_start_event()),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::text(""),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::TextDelta {
                    text: "Hello".into(),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::TextDelta {
                    text: " world".into(),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
                index: 0,
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::MessageStop)),
        ];

        let stream = EventStream::from_events(events).on_text_delta(move |chunk| {
            sink.lock().unwrap().push(chunk.to_string());
        });
        stream.aggregate().await.unwrap();

        assert_eq!(*captured.lock().unwrap(), vec!["Hello", " world"]);
    }

    #[tokio::test]
    async fn on_tool_use_complete_fires_with_parsed_input() {
        let captured: Arc<Mutex<Vec<(String, String, serde_json::Value)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let events = vec![
            Ok(message_start_event()),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Known(KnownBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "get_weather".into(),
                    input: json!({}),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::InputJsonDelta {
                    partial_json: "{\"city\":\"Paris\"}".into(),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
                index: 0,
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::MessageStop)),
        ];

        let stream =
            EventStream::from_events(events).on_tool_use_complete(move |id, name, input| {
                sink.lock()
                    .unwrap()
                    .push((id.to_string(), name.to_string(), input.clone()));
            });
        stream.aggregate().await.unwrap();

        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "toolu_1");
        assert_eq!(captured[0].1, "get_weather");
        assert_eq!(captured[0].2, json!({"city": "Paris"}));
    }

    #[tokio::test]
    async fn on_tool_use_complete_fires_for_server_tool_use_blocks() {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let events = vec![
            Ok(message_start_event()),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Known(KnownBlock::ServerToolUse {
                    id: "srvu_1".into(),
                    name: "web_search".into(),
                    input: json!({}),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::InputJsonDelta {
                    partial_json: "{\"q\":\"rust\"}".into(),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
                index: 0,
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::MessageStop)),
        ];

        let stream = EventStream::from_events(events).on_tool_use_complete(move |id, _, _| {
            sink.lock().unwrap().push(id.to_string());
        });
        stream.aggregate().await.unwrap();

        assert_eq!(*captured.lock().unwrap(), vec!["srvu_1"]);
    }

    #[tokio::test]
    async fn on_thinking_delta_fires_for_each_thinking_chunk() {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let events = vec![
            Ok(message_start_event()),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Known(KnownBlock::Thinking {
                    thinking: String::new(),
                    signature: String::new(),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::ThinkingDelta {
                    thinking: "let me ".into(),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::ThinkingDelta {
                    thinking: "think".into(),
                }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
                index: 0,
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::MessageStop)),
        ];

        let stream = EventStream::from_events(events).on_thinking_delta(move |chunk| {
            sink.lock().unwrap().push(chunk.to_string());
        });
        stream.aggregate().await.unwrap();

        assert_eq!(*captured.lock().unwrap(), vec!["let me ", "think"]);
    }

    #[tokio::test]
    async fn on_message_stop_fires_once_with_usage() {
        let captured: Arc<Mutex<Vec<Usage>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let events = vec![
            Ok(message_start_event()),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::text(""),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::TextDelta { text: "hi".into() }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
                index: 0,
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some(StopReason::EndTurn),
                    stop_sequence: None,
                },
                usage: Usage {
                    input_tokens: 5,
                    output_tokens: 7,
                    ..Usage::default()
                },
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::MessageStop)),
        ];

        let stream = EventStream::from_events(events).on_message_stop(move |usage| {
            sink.lock().unwrap().push(usage.clone());
        });
        stream.aggregate().await.unwrap();

        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].input_tokens, 5);
        assert_eq!(captured[0].output_tokens, 7);
    }

    #[tokio::test]
    async fn on_error_fires_before_propagating_server_error() {
        let count = Arc::new(Mutex::new(0u32));
        let sink = Arc::clone(&count);
        let events = vec![
            Ok(message_start_event()),
            Ok(StreamEvent::Known(KnownStreamEvent::Error {
                error: ApiErrorPayload {
                    kind: ApiErrorKind::OverloadedError,
                    message: "boom".into(),
                },
            })),
        ];

        let stream = EventStream::from_events(events).on_error(move |_| {
            *sink.lock().unwrap() += 1;
        });
        let err = stream.aggregate().await.unwrap_err();
        assert!(matches!(
            err,
            Error::Stream(StreamError::Server {
                kind: ApiErrorKind::OverloadedError,
                ..
            })
        ));
        assert_eq!(
            *count.lock().unwrap(),
            1,
            "handler should fire exactly once"
        );
    }

    #[tokio::test]
    async fn on_error_fires_for_transport_error() {
        let count = Arc::new(Mutex::new(0u32));
        let sink = Arc::clone(&count);
        let events: Vec<Result<StreamEvent>> = vec![
            Ok(message_start_event()),
            Err(Error::Stream(StreamError::Parse("decode failed".into()))),
        ];

        let stream = EventStream::from_events(events).on_error(move |_| {
            *sink.lock().unwrap() += 1;
        });
        let err = stream.aggregate().await.unwrap_err();
        assert!(matches!(err, Error::Stream(StreamError::Parse(_))));
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn raw_stream_iteration_does_not_fire_callbacks() {
        use futures_util::StreamExt;
        let count = Arc::new(Mutex::new(0u32));
        let sink = Arc::clone(&count);
        let events = vec![
            Ok(message_start_event()),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::text(""),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Known(KnownContentDelta::TextDelta { text: "hi".into() }),
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::ContentBlockStop {
                index: 0,
            })),
            Ok(StreamEvent::Known(KnownStreamEvent::MessageStop)),
        ];

        let mut stream = EventStream::from_events(events).on_text_delta(move |_| {
            *sink.lock().unwrap() += 1;
        });
        while let Some(_ev) = stream.next().await {}
        // Callbacks only fire during aggregate(), not raw .next().
        assert_eq!(*count.lock().unwrap(), 0);
    }
}
