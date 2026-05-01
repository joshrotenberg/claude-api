//! Session events: typed user / agent / session / span events.
//!
//! Communication with a session is event-based. You send `user.*`
//! events and receive `agent.*`, `session.*`, and `span.*` events back.
//! Every event in this module is forward-compatible: an unknown wire
//! `type` tag falls through to [`SessionEvent::Other`] preserving the
//! raw JSON, so brand-new server variants don't break the build.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::forward_compat::dispatch_known_or_other;
use crate::pagination::Paginated;

use super::MANAGED_AGENTS_BETA;

// =====================================================================
// Wire envelope
// =====================================================================

/// One event on a Managed Agents session.
///
/// Forward-compatible: known types deserialize into [`Self::Known`];
/// unrecognized wire `type` tags land in [`Self::Other`] preserving the
/// raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// Recognized event.
    Known(KnownSessionEvent),
    /// Unknown event type; the raw JSON is preserved.
    Other(serde_json::Value),
}

/// `type` tags this SDK version recognizes for incoming session events.
const KNOWN_INCOMING_TAGS: &[&str] = &[
    // Agent
    "agent.message",
    "agent.thinking",
    "agent.tool_use",
    "agent.tool_result",
    "agent.mcp_tool_use",
    "agent.mcp_tool_result",
    "agent.custom_tool_use",
    "agent.thread_context_compacted",
    "agent.thread_message_sent",
    "agent.thread_message_received",
    // Session
    "session.status_running",
    "session.status_idle",
    "session.status_rescheduled",
    "session.status_terminated",
    "session.deleted",
    "session.error",
    "session.outcome_evaluated",
    "session.thread_created",
    "session.thread_idle",
    // Span
    "span.model_request_start",
    "span.model_request_end",
    "span.outcome_evaluation_start",
    "span.outcome_evaluation_ongoing",
    "span.outcome_evaluation_end",
    // User events are also visible on history reads, so include them.
    "user.message",
    "user.interrupt",
    "user.custom_tool_result",
    "user.tool_confirmation",
    "user.define_outcome",
];

impl Serialize for SessionEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Known(k) => k.serialize(s),
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        dispatch_known_or_other(
            raw,
            KNOWN_INCOMING_TAGS,
            SessionEvent::Known,
            SessionEvent::Other,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl SessionEvent {
    /// If this is a known event, return the inner [`KnownSessionEvent`].
    #[must_use]
    pub fn known(&self) -> Option<&KnownSessionEvent> {
        match self {
            Self::Known(k) => Some(k),
            Self::Other(_) => None,
        }
    }

    /// Wire-level `type` tag for this event regardless of variant.
    #[must_use]
    pub fn type_tag(&self) -> Option<String> {
        match self {
            Self::Known(k) => serde_json::to_value(k).ok().and_then(|v| {
                v.get("type")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            }),
            Self::Other(v) => v
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(String::from),
        }
    }
}

// =====================================================================
// Known event union
// =====================================================================

/// All event variants this SDK version recognizes.
///
/// Common envelope fields (`id`, `processed_at`) are present on most
/// events; we capture them as optional fields when the server includes
/// them and let new fields land in `Other` via the parent enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum KnownSessionEvent {
    // -----------------------------------------------------------------
    // Agent events
    // -----------------------------------------------------------------
    /// Agent response containing text content blocks.
    #[serde(rename = "agent.message")]
    AgentMessage(AgentMessageEvent),
    /// Agent thinking content, emitted separately from messages.
    #[serde(rename = "agent.thinking")]
    AgentThinking(AgentThinkingEvent),
    /// Agent invokes a pre-built agent tool (bash, file ops, etc.).
    #[serde(rename = "agent.tool_use")]
    AgentToolUse(AgentToolUseEvent),
    /// Result of a pre-built agent tool execution.
    #[serde(rename = "agent.tool_result")]
    AgentToolResult(AgentToolResultEvent),
    /// Agent invokes an MCP server tool.
    #[serde(rename = "agent.mcp_tool_use")]
    AgentMcpToolUse(AgentMcpToolUseEvent),
    /// Result of an MCP tool execution.
    #[serde(rename = "agent.mcp_tool_result")]
    AgentMcpToolResult(AgentMcpToolResultEvent),
    /// Agent invokes one of your custom tools. Respond with
    /// [`UserCustomToolResult`].
    #[serde(rename = "agent.custom_tool_use")]
    AgentCustomToolUse(AgentCustomToolUseEvent),
    /// Conversation history was compacted.
    #[serde(rename = "agent.thread_context_compacted")]
    AgentThreadContextCompacted(EventEnvelope),
    /// Agent sent a message to another multi-agent thread.
    #[serde(rename = "agent.thread_message_sent")]
    AgentThreadMessageSent(AgentThreadMessageSentEvent),
    /// Agent received a message from another multi-agent thread.
    #[serde(rename = "agent.thread_message_received")]
    AgentThreadMessageReceived(AgentThreadMessageReceivedEvent),

    // -----------------------------------------------------------------
    // Session events
    // -----------------------------------------------------------------
    /// Session is now actively processing.
    #[serde(rename = "session.status_running")]
    SessionStatusRunning(EventEnvelope),
    /// Session finished its current task and is waiting for input.
    #[serde(rename = "session.status_idle")]
    SessionStatusIdle(SessionStatusIdleEvent),
    /// Transient error; session is auto-retrying.
    #[serde(rename = "session.status_rescheduled")]
    SessionStatusRescheduled(EventEnvelope),
    /// Session ended due to an unrecoverable error.
    #[serde(rename = "session.status_terminated")]
    SessionStatusTerminated(EventEnvelope),
    /// Session was deleted; emitted as the final event before the
    /// session disappears from listings.
    #[serde(rename = "session.deleted")]
    SessionDeleted(EventEnvelope),
    /// An error occurred during processing.
    #[serde(rename = "session.error")]
    SessionError(SessionErrorEvent),
    /// An outcome evaluation reached a terminal status.
    #[serde(rename = "session.outcome_evaluated")]
    SessionOutcomeEvaluated(EventEnvelope),
    /// Coordinator spawned a new multi-agent thread.
    #[serde(rename = "session.thread_created")]
    SessionThreadCreated(SessionThreadCreatedEvent),
    /// A multi-agent thread finished its current work.
    #[serde(rename = "session.thread_idle")]
    SessionThreadIdle(EventEnvelope),

    // -----------------------------------------------------------------
    // Span events
    // -----------------------------------------------------------------
    /// A model inference call has started.
    #[serde(rename = "span.model_request_start")]
    SpanModelRequestStart(EventEnvelope),
    /// A model inference call has completed.
    #[serde(rename = "span.model_request_end")]
    SpanModelRequestEnd(SpanModelRequestEndEvent),
    /// Outcome evaluation has started.
    #[serde(rename = "span.outcome_evaluation_start")]
    SpanOutcomeEvaluationStart(EventEnvelope),
    /// Heartbeat during an ongoing outcome evaluation.
    #[serde(rename = "span.outcome_evaluation_ongoing")]
    SpanOutcomeEvaluationOngoing(EventEnvelope),
    /// Outcome evaluation has completed.
    #[serde(rename = "span.outcome_evaluation_end")]
    SpanOutcomeEvaluationEnd(EventEnvelope),

    // -----------------------------------------------------------------
    // User events (echoed on history reads)
    // -----------------------------------------------------------------
    /// User-authored text message.
    #[serde(rename = "user.message")]
    UserMessage(UserMessageEvent),
    /// User-issued interrupt (no body beyond envelope).
    #[serde(rename = "user.interrupt")]
    UserInterrupt(EventEnvelope),
    /// Result of a custom tool the user executed locally.
    #[serde(rename = "user.custom_tool_result")]
    UserCustomToolResult(UserCustomToolResultEvent),
    /// Allow / deny a pending agent or MCP tool call.
    #[serde(rename = "user.tool_confirmation")]
    UserToolConfirmation(UserToolConfirmationEvent),
    /// Define an outcome for the agent to work toward.
    #[serde(rename = "user.define_outcome")]
    UserDefineOutcome(UserDefineOutcomeEvent),
}

// =====================================================================
// Per-variant payload structs
// =====================================================================

/// Common envelope fields present on most events. Used as the body of
/// any event that has no additional payload beyond the envelope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EventEnvelope {
    /// Server-assigned event ID (`sevt_...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp. `None` if the event is queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
}

/// `agent.message`: text content blocks the agent emitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentMessageEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Content blocks. Captured as raw JSON; the same `ContentBlock`
    /// shape from the messages API applies, but we don't pin to it
    /// here so a divergent server-side schema doesn't break parsing.
    pub content: Vec<serde_json::Value>,
}

/// `agent.thinking`: extended-thinking content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentThinkingEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Thinking text.
    #[serde(default)]
    pub thinking: String,
    /// Optional signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// `agent.tool_use`: agent invokes a pre-built tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentToolUseEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Tool name.
    pub name: String,
    /// Tool input.
    pub input: serde_json::Value,
    /// Set on multi-agent sessions when the request originated in a
    /// sub-agent thread. Echo this on the corresponding
    /// [`OutgoingUserEvent::ToolConfirmation`] or
    /// [`OutgoingUserEvent::CustomToolResult`] reply so the platform
    /// routes it back to the waiting thread. Absent for primary-thread
    /// events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_thread_id: Option<String>,
}

/// `agent.tool_result`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentToolResultEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// ID of the matching `agent.tool_use` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// Tool result content.
    #[serde(default)]
    pub content: serde_json::Value,
    /// `true` if the tool reported an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// `agent.mcp_tool_use`: agent invokes an MCP server tool.
pub type AgentMcpToolUseEvent = AgentToolUseEvent;

/// `agent.mcp_tool_result`.
pub type AgentMcpToolResultEvent = AgentToolResultEvent;

/// `agent.custom_tool_use`: agent invokes one of the caller's custom
/// tools. The session pauses; respond with [`UserCustomToolResult`].
pub type AgentCustomToolUseEvent = AgentToolUseEvent;

/// `session.thread_created`. Carries the new thread's ID and the
/// model the spawned agent runs against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionThreadCreatedEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Newly-spawned thread ID (`sthr_...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_thread_id: Option<String>,
    /// Model the spawned agent runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// `agent.thread_message_sent`. The agent sent a message to another
/// thread (typically the coordinator delegating to a sub-agent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentThreadMessageSentEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Destination thread ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_thread_id: Option<String>,
    /// Message content as raw JSON (typed shape may evolve).
    #[serde(default)]
    pub content: serde_json::Value,
}

/// `agent.thread_message_received`. The agent received a message from
/// another thread (typically a sub-agent responding to the
/// coordinator).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentThreadMessageReceivedEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Source thread ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_thread_id: Option<String>,
    /// Message content as raw JSON.
    #[serde(default)]
    pub content: serde_json::Value,
}

/// `session.status_idle`. Carries an optional `stop_reason` describing
/// why the agent paused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionStatusIdleEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Why the session went idle. `None` if the server didn't send one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

/// Reason the session went idle. Forward-compatible: unknown `type`
/// tags fall through to [`Self::Other`].
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Recognized stop reason.
    Known(KnownStopReason),
    /// Unknown stop reason; raw JSON preserved.
    Other(serde_json::Value),
}

/// Stop-reason variants this SDK version knows about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnownStopReason {
    /// Agent finished its turn naturally.
    EndTurn,
    /// One or more tool/confirmation events are blocking; the
    /// session waits for `user.tool_confirmation` or
    /// `user.custom_tool_result` keyed off the listed event IDs.
    RequiresAction {
        /// Event IDs the session is waiting on.
        event_ids: Vec<String>,
    },
    /// Hit a configured stop sequence.
    StopSequence,
    /// Reached the max-tokens cap.
    MaxTokens,
}

const KNOWN_STOP_REASON_TAGS: &[&str] =
    &["end_turn", "requires_action", "stop_sequence", "max_tokens"];

impl Serialize for StopReason {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Known(k) => k.serialize(s),
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for StopReason {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        dispatch_known_or_other(
            raw,
            KNOWN_STOP_REASON_TAGS,
            StopReason::Known,
            StopReason::Other,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// `session.error`. Includes a typed error payload with retry status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionErrorEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// The error payload. Captured as raw JSON until the schema
    /// stabilizes (the docs show a `retry_status` field but don't
    /// enumerate its values).
    #[serde(default)]
    pub error: serde_json::Value,
}

/// `span.model_request_end`. Includes `model_usage` with token counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpanModelRequestEndEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// ID of the matching `span.model_request_start` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_request_start_id: Option<String>,
    /// Model usage counts. Matches the [`SessionUsage`](super::sessions::SessionUsage)
    /// shape but at finer per-call granularity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_usage: Option<crate::types::Usage>,
}

/// `user.message`: a text user message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserMessageEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Content blocks. The simplest form is `[{"type":"text","text":"..."}]`.
    pub content: Vec<UserContentBlock>,
}

/// One block of content inside a [`UserMessageEvent`] or
/// [`UserCustomToolResultEvent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum UserContentBlock {
    /// Plain text.
    Text {
        /// Text body.
        text: String,
    },
}

impl UserContentBlock {
    /// Convenience: build a `text` block.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

/// `user.custom_tool_result`: caller-side response to an
/// `agent.custom_tool_use`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserCustomToolResultEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// ID of the matching `agent.custom_tool_use` event.
    pub custom_tool_use_id: String,
    /// Result content.
    pub content: Vec<UserContentBlock>,
    /// Optional error flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// `user.tool_confirmation`: allow / deny a pending tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserToolConfirmationEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// ID of the matching `agent.tool_use` or `agent.mcp_tool_use` event.
    pub tool_use_id: String,
    /// Verdict.
    pub result: ConfirmationResult,
    /// Optional message to surface to the agent on a `Deny`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny_message: Option<String>,
}

/// `allow` / `deny` verdict for [`UserToolConfirmationEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConfirmationResult {
    /// Run the pending tool call.
    Allow,
    /// Skip the pending tool call. Use `deny_message` to explain.
    Deny,
}

/// `user.define_outcome`: define an outcome the agent works toward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserDefineOutcomeEvent {
    /// Envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Server-side recording timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<String>,
    /// Server-assigned outcome ID, present on echoed events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_id: Option<String>,
    /// Human description of the desired outcome.
    pub description: String,
    /// Rubric: inline text or a Files API reference.
    pub rubric: OutcomeRubric,
    /// Maximum revision iterations. Defaults to 3, capped at 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
}

/// Rubric source for an outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum OutcomeRubric {
    /// Inline rubric text.
    Text {
        /// Rubric body.
        content: String,
    },
    /// Rubric stored as a [`File`](crate::files::FileMetadata).
    File {
        /// Files API ID.
        file_id: String,
    },
}

// =====================================================================
// Send-events request shape
// =====================================================================

/// One event included in a [`Sessions::events_send`] call.
///
/// This is the *outgoing* form -- the user-event variants only. For the
/// echoed / received form (which can also carry agent / session / span
/// events) use [`SessionEvent`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum OutgoingUserEvent {
    /// Send a user message.
    #[serde(rename = "user.message")]
    Message {
        /// Content blocks.
        content: Vec<UserContentBlock>,
    },
    /// Interrupt the agent mid-execution.
    #[serde(rename = "user.interrupt")]
    Interrupt {},
    /// Respond to an `agent.custom_tool_use`.
    #[serde(rename = "user.custom_tool_result")]
    CustomToolResult {
        /// ID of the matching `agent.custom_tool_use` event.
        custom_tool_use_id: String,
        /// Result content.
        content: Vec<UserContentBlock>,
        /// Optional error flag.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        /// Multi-agent routing: set to the value from the originating
        /// `agent.custom_tool_use` event's `session_thread_id` field
        /// when responding to a sub-agent thread. Leave `None` for
        /// primary-thread events.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_thread_id: Option<String>,
    },
    /// Allow or deny a pending tool call.
    #[serde(rename = "user.tool_confirmation")]
    ToolConfirmation {
        /// ID of the matching `agent.tool_use` event.
        tool_use_id: String,
        /// Allow or deny.
        result: ConfirmationResult,
        /// Optional explanation for a deny.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deny_message: Option<String>,
        /// Multi-agent routing: set to the originating event's
        /// `session_thread_id`. Leave `None` for primary-thread events.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_thread_id: Option<String>,
    },
    /// Define an outcome.
    #[serde(rename = "user.define_outcome")]
    DefineOutcome(UserDefineOutcomeEvent),
}

impl OutgoingUserEvent {
    /// Build a simple `user.message` from a single text string.
    #[must_use]
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message {
            content: vec![UserContentBlock::text(text)],
        }
    }

    /// Build a `user.interrupt`.
    #[must_use]
    pub fn interrupt() -> Self {
        Self::Interrupt {}
    }

    /// Build a `user.tool_confirmation` (allow).
    #[must_use]
    pub fn allow_tool(tool_use_id: impl Into<String>) -> Self {
        Self::ToolConfirmation {
            tool_use_id: tool_use_id.into(),
            result: ConfirmationResult::Allow,
            deny_message: None,
            session_thread_id: None,
        }
    }

    /// Build a `user.tool_confirmation` (deny with message).
    #[must_use]
    pub fn deny_tool(tool_use_id: impl Into<String>, deny_message: impl Into<String>) -> Self {
        Self::ToolConfirmation {
            tool_use_id: tool_use_id.into(),
            result: ConfirmationResult::Deny,
            deny_message: Some(deny_message.into()),
            session_thread_id: None,
        }
    }

    /// Build a `user.custom_tool_result` with a single text block.
    #[must_use]
    pub fn custom_tool_result_text(
        custom_tool_use_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self::CustomToolResult {
            custom_tool_use_id: custom_tool_use_id.into(),
            content: vec![UserContentBlock::text(text)],
            is_error: None,
            session_thread_id: None,
        }
    }

    /// Attach a `session_thread_id` to a `ToolConfirmation` or
    /// `CustomToolResult` event for multi-agent thread routing. No-op
    /// on other variants.
    #[must_use]
    pub fn with_session_thread_id(mut self, thread_id: impl Into<String>) -> Self {
        let id = thread_id.into();
        match &mut self {
            Self::ToolConfirmation {
                session_thread_id, ..
            }
            | Self::CustomToolResult {
                session_thread_id, ..
            } => {
                *session_thread_id = Some(id);
            }
            Self::Message { .. } | Self::Interrupt {} | Self::DefineOutcome(_) => {}
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
struct SendEventsRequest<'a> {
    events: &'a [OutgoingUserEvent],
}

// =====================================================================
// Namespace handle (events.send / events.list)
// =====================================================================

/// Namespace handle for session-events operations.
///
/// Obtained via
/// [`Sessions::events`](super::sessions::Sessions::events).
pub struct Events<'a> {
    pub(crate) client: &'a Client,
    pub(crate) session_id: String,
}

impl Events<'_> {
    /// `POST /v1/sessions/{id}/events`. The server returns 204 on
    /// success; this method returns `()`.
    pub async fn send(&self, events: &[OutgoingUserEvent]) -> Result<()> {
        let path = format!("/v1/sessions/{}/events", self.session_id);
        let body = SendEventsRequest { events };
        let _: serde_json::Value = self
            .client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(&body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await?;
        Ok(())
    }

    /// `GET /v1/sessions/{id}/events`. Returns the full event history
    /// for the session as a [`Paginated<SessionEvent>`].
    pub async fn list(&self) -> Result<Paginated<SessionEvent>> {
        let path = format!("/v1/sessions/{}/events", self.session_id);
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/sessions/{id}/stream`. Returns an
    /// [`EventStream`](crate::managed_agents::events::EventStream)
    /// yielding [`SessionEvent`]s as they're emitted server-side.
    ///
    /// **Open the stream before sending events** to avoid a race: only
    /// events emitted *after* the stream is opened are delivered. To
    /// reconnect to an existing session without missing events, open
    /// the stream first, then [`list`](Self::list) the history to seed
    /// a set of seen event IDs and skip duplicates from the live tail.
    ///
    /// Streaming requests are *not* retried -- a mid-stream retry
    /// would silently drop events.
    #[cfg(feature = "streaming")]
    #[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
    pub async fn stream(&self) -> Result<EventStream> {
        let path = format!("/v1/sessions/{}/stream", self.session_id);
        let response = self
            .client
            .execute_streaming(
                self.client
                    .request_builder(reqwest::Method::GET, &path)
                    .header("accept", "text/event-stream"),
                &[MANAGED_AGENTS_BETA],
            )
            .await?;
        Ok(EventStream::from_response(response))
    }
}

// =====================================================================
// Streaming event stream
// =====================================================================

/// SSE-backed stream of [`SessionEvent`]s for a Managed Agents session.
///
/// Obtain via [`Events::stream`]. Iterate as a `futures_util::Stream`:
///
/// ```ignore
/// use futures_util::StreamExt;
/// let mut stream = client
///     .managed_agents()
///     .sessions()
///     .events("sesn_x")
///     .stream()
///     .await?;
/// while let Some(event) = stream.next().await {
///     match event? {
///         SessionEvent::Known(KnownSessionEvent::AgentMessage(m)) => {
///             // handle text deltas
///         }
///         _ => {}
///     }
/// }
/// ```
#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
pub struct EventStream {
    inner: futures_util::stream::BoxStream<'static, Result<SessionEvent>>,
}

#[cfg(feature = "streaming")]
impl EventStream {
    /// Wrap a streaming HTTP response into a typed event stream.
    pub(crate) fn from_response(response: reqwest::Response) -> Self {
        use futures_util::StreamExt;
        Self {
            inner: crate::sse::into_typed_stream::<SessionEvent>(response).boxed(),
        }
    }
}

#[cfg(feature = "streaming")]
impl futures_util::Stream for EventStream {
    type Item = Result<SessionEvent>;

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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[test]
    fn known_agent_message_round_trips() {
        let raw = json!({
            "type": "agent.message",
            "id": "sevt_01",
            "processed_at": "2026-04-30T12:00:00Z",
            "content": [{"type": "text", "text": "hello"}]
        });
        let ev: SessionEvent = serde_json::from_value(raw.clone()).unwrap();
        match &ev {
            SessionEvent::Known(KnownSessionEvent::AgentMessage(m)) => {
                assert_eq!(m.id.as_deref(), Some("sevt_01"));
                assert_eq!(m.content.len(), 1);
            }
            other => panic!("expected AgentMessage, got {other:?}"),
        }
        // Round-trip preserves shape (allowing field reordering).
        let back = serde_json::to_value(&ev).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn unknown_event_type_falls_through_to_other() {
        let raw = json!({
            "type": "agent.future_event",
            "id": "sevt_99",
            "extra": [1, 2, 3]
        });
        let ev: SessionEvent = serde_json::from_value(raw.clone()).unwrap();
        match &ev {
            SessionEvent::Other(v) => assert_eq!(v, &raw),
            SessionEvent::Known(_) => panic!("expected Other, got Known: {ev:?}"),
        }
        // Round-trip.
        assert_eq!(serde_json::to_value(&ev).unwrap(), raw);
        assert_eq!(ev.type_tag().as_deref(), Some("agent.future_event"));
    }

    #[test]
    fn malformed_known_event_errors() {
        // type matches "agent.tool_use" but `input` is missing.
        let raw = json!({"type": "agent.tool_use", "name": "bash"});
        let parsed: std::result::Result<SessionEvent, _> = serde_json::from_value(raw);
        assert!(parsed.is_err(), "must not silently fall through to Other");
    }

    #[test]
    fn session_status_idle_with_requires_action_decodes_event_ids() {
        let raw = json!({
            "type": "session.status_idle",
            "id": "sevt_77",
            "stop_reason": {
                "type": "requires_action",
                "event_ids": ["sevt_a", "sevt_b"]
            }
        });
        let ev: SessionEvent = serde_json::from_value(raw).unwrap();
        let SessionEvent::Known(KnownSessionEvent::SessionStatusIdle(idle)) = &ev else {
            panic!("expected SessionStatusIdle, got {ev:?}");
        };
        let StopReason::Known(KnownStopReason::RequiresAction { event_ids }) =
            idle.stop_reason.as_ref().unwrap()
        else {
            panic!("expected RequiresAction stop reason");
        };
        assert_eq!(event_ids, &["sevt_a", "sevt_b"]);
    }

    #[test]
    fn session_status_idle_with_unknown_stop_reason_lands_in_other() {
        let raw = json!({
            "type": "session.status_idle",
            "stop_reason": {"type": "future_reason", "x": 1}
        });
        let ev: SessionEvent = serde_json::from_value(raw).unwrap();
        let SessionEvent::Known(KnownSessionEvent::SessionStatusIdle(idle)) = &ev else {
            panic!("expected SessionStatusIdle");
        };
        match idle.stop_reason.as_ref().unwrap() {
            StopReason::Other(v) => assert_eq!(v["type"], "future_reason"),
            StopReason::Known(_) => panic!("expected Other stop reason, got Known"),
        }
    }

    #[test]
    fn outgoing_user_message_serializes_with_text_block() {
        let ev = OutgoingUserEvent::message("hi");
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "user.message",
                "content": [{"type": "text", "text": "hi"}]
            })
        );
    }

    #[test]
    fn outgoing_user_interrupt_serializes_minimal_object() {
        let ev = OutgoingUserEvent::interrupt();
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v, json!({"type": "user.interrupt"}));
    }

    #[test]
    fn outgoing_tool_confirmation_serializes_allow_and_deny() {
        let allow = OutgoingUserEvent::allow_tool("sevt_1");
        assert_eq!(
            serde_json::to_value(&allow).unwrap(),
            json!({
                "type": "user.tool_confirmation",
                "tool_use_id": "sevt_1",
                "result": "allow"
            })
        );

        let deny = OutgoingUserEvent::deny_tool("sevt_2", "policy violation");
        assert_eq!(
            serde_json::to_value(&deny).unwrap(),
            json!({
                "type": "user.tool_confirmation",
                "tool_use_id": "sevt_2",
                "result": "deny",
                "deny_message": "policy violation"
            })
        );
    }

    #[test]
    fn session_thread_created_event_decodes_thread_id_and_model() {
        let raw = json!({
            "type": "session.thread_created",
            "id": "sevt_1",
            "session_thread_id": "sthr_a",
            "model": "claude-opus-4-7"
        });
        let ev: SessionEvent = serde_json::from_value(raw).unwrap();
        let SessionEvent::Known(KnownSessionEvent::SessionThreadCreated(t)) = ev else {
            panic!("expected SessionThreadCreated");
        };
        assert_eq!(t.session_thread_id.as_deref(), Some("sthr_a"));
        assert_eq!(t.model.as_deref(), Some("claude-opus-4-7"));
    }

    #[test]
    fn agent_thread_message_sent_event_decodes_to_thread_id() {
        let raw = json!({
            "type": "agent.thread_message_sent",
            "id": "sevt_2",
            "to_thread_id": "sthr_b",
            "content": [{"type": "text", "text": "delegate"}]
        });
        let ev: SessionEvent = serde_json::from_value(raw).unwrap();
        let SessionEvent::Known(KnownSessionEvent::AgentThreadMessageSent(m)) = ev else {
            panic!("expected AgentThreadMessageSent");
        };
        assert_eq!(m.to_thread_id.as_deref(), Some("sthr_b"));
    }

    #[test]
    fn agent_thread_message_received_event_decodes_from_thread_id() {
        let raw = json!({
            "type": "agent.thread_message_received",
            "id": "sevt_3",
            "from_thread_id": "sthr_b",
            "content": [{"type": "text", "text": "done"}]
        });
        let ev: SessionEvent = serde_json::from_value(raw).unwrap();
        let SessionEvent::Known(KnownSessionEvent::AgentThreadMessageReceived(m)) = ev else {
            panic!("expected AgentThreadMessageReceived");
        };
        assert_eq!(m.from_thread_id.as_deref(), Some("sthr_b"));
    }

    #[test]
    fn agent_tool_use_event_carries_session_thread_id_when_in_subagent_thread() {
        let raw = json!({
            "type": "agent.tool_use",
            "id": "sevt_4",
            "name": "bash",
            "input": {"cmd": "ls"},
            "session_thread_id": "sthr_b"
        });
        let ev: SessionEvent = serde_json::from_value(raw).unwrap();
        let SessionEvent::Known(KnownSessionEvent::AgentToolUse(t)) = ev else {
            panic!("expected AgentToolUse");
        };
        assert_eq!(t.session_thread_id.as_deref(), Some("sthr_b"));
    }

    #[test]
    fn outgoing_tool_confirmation_with_thread_id_routes_reply() {
        let ev = OutgoingUserEvent::allow_tool("sevt_4").with_session_thread_id("sthr_b");
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["session_thread_id"], "sthr_b");
        assert_eq!(v["type"], "user.tool_confirmation");
    }

    #[test]
    fn outgoing_custom_tool_result_with_thread_id_routes_reply() {
        let ev = OutgoingUserEvent::custom_tool_result_text("sevt_5", "ok")
            .with_session_thread_id("sthr_c");
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["session_thread_id"], "sthr_c");
        assert_eq!(v["custom_tool_use_id"], "sevt_5");
    }

    #[test]
    fn outgoing_tool_confirmation_without_thread_id_omits_field() {
        let ev = OutgoingUserEvent::allow_tool("sevt_4");
        let v = serde_json::to_value(&ev).unwrap();
        assert!(v.get("session_thread_id").is_none(), "{v}");
    }

    #[tokio::test]
    async fn events_send_posts_to_events_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/sessions/sesn_x/events"))
            .and(header("anthropic-beta", "managed-agents-2026-04-01"))
            .and(body_partial_json(json!({
                "events": [
                    {"type": "user.message", "content": [{"type": "text", "text": "go"}]}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        client
            .managed_agents()
            .sessions()
            .events("sesn_x")
            .send(&[OutgoingUserEvent::message("go")])
            .await
            .unwrap();
    }

    #[cfg(feature = "streaming")]
    #[tokio::test]
    async fn events_stream_yields_typed_session_events() {
        use futures_util::StreamExt;
        let sse_body = concat!(
            "event: message\n",
            "data: {\"type\":\"agent.message\",\"id\":\"sevt_1\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}\n",
            "\n",
            "event: message\n",
            "data: {\"type\":\"session.status_idle\",\"id\":\"sevt_2\",\"stop_reason\":{\"type\":\"end_turn\"}}\n",
            "\n",
        );

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_x/stream"))
            .and(header("anthropic-beta", "managed-agents-2026-04-01"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut stream = client
            .managed_agents()
            .sessions()
            .events("sesn_x")
            .stream()
            .await
            .unwrap();

        let first = stream.next().await.unwrap().unwrap();
        assert!(matches!(
            first,
            SessionEvent::Known(KnownSessionEvent::AgentMessage(_))
        ));

        let second = stream.next().await.unwrap().unwrap();
        let SessionEvent::Known(KnownSessionEvent::SessionStatusIdle(idle)) = second else {
            panic!("expected SessionStatusIdle");
        };
        assert!(matches!(
            idle.stop_reason,
            Some(StopReason::Known(KnownStopReason::EndTurn))
        ));
    }

    #[cfg(feature = "streaming")]
    #[tokio::test]
    async fn events_stream_propagates_unauthorized_response() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_x/stream"))
            .respond_with(
                ResponseTemplate::new(401)
                    .insert_header("request-id", "req_unauth")
                    .set_body_json(json!({
                        "type": "error",
                        "error": {"type": "authentication_error", "message": "bad key"}
                    })),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let err = client
            .managed_agents()
            .sessions()
            .events("sesn_x")
            .stream()
            .await
            .unwrap_err();
        assert_eq!(err.status(), Some(http::StatusCode::UNAUTHORIZED));
        assert_eq!(err.request_id(), Some("req_unauth"));
    }

    #[tokio::test]
    async fn events_list_returns_paginated_event_stream() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_x/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"type": "user.message", "content": [{"type": "text", "text": "hi"}]},
                    {"type": "agent.message", "content": [{"type": "text", "text": "hello"}]}
                ],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .sessions()
            .events("sesn_x")
            .list()
            .await
            .unwrap();
        assert_eq!(page.data.len(), 2);
        assert!(matches!(
            page.data[0],
            SessionEvent::Known(KnownSessionEvent::UserMessage(_))
        ));
        assert!(matches!(
            page.data[1],
            SessionEvent::Known(KnownSessionEvent::AgentMessage(_))
        ));
    }
}
