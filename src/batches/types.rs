//! Wire types for the Batches API.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::ApiErrorPayload;
use crate::messages::request::CreateMessageRequest;
use crate::messages::response::Message;

/// One entry in a batch submission.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct BatchRequest {
    /// Caller-chosen identifier the response will echo back. Must be
    /// unique within the batch; used to correlate results to inputs.
    pub custom_id: String,
    /// The Messages-API request payload for this entry.
    pub params: CreateMessageRequest,
}

impl BatchRequest {
    /// Construct a new batch entry.
    #[must_use]
    pub fn new(custom_id: impl Into<String>, params: CreateMessageRequest) -> Self {
        Self {
            custom_id: custom_id.into(),
            params,
        }
    }
}

/// Status snapshot of a batch from `GET /v1/messages/batches/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessageBatch {
    /// Stable batch identifier (e.g. `msgbatch_01...`).
    pub id: String,
    /// Wire `type` discriminant; always `"message_batch"`.
    #[serde(rename = "type", default = "default_batch_kind")]
    pub kind: String,
    /// Current processing status.
    pub processing_status: ProcessingStatus,
    /// Per-status counts of the batch entries.
    pub request_counts: RequestCounts,
    /// Creation timestamp (ISO-8601).
    pub created_at: String,
    /// Expiration timestamp (ISO-8601).
    pub expires_at: String,
    /// Set once processing is complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    /// Set when the batch is archived by the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// Set when a cancel was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_initiated_at: Option<String>,
    /// URL hosting the JSONL results, available once `ended_at` is set.
    /// The SDK does not require this directly -- use [`super::Batches::results`]
    /// or [`super::Batches::results_stream`] to fetch them by batch id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub results_url: Option<String>,
}

fn default_batch_kind() -> String {
    "message_batch".to_owned()
}

/// Where a batch is in its processing lifecycle.
///
/// Forward-compatible: unknown values deserialize to
/// [`ProcessingStatus::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProcessingStatus {
    /// Batch is currently running.
    InProgress,
    /// Cancel was requested but in-flight requests haven't terminated.
    Canceling,
    /// All entries reached a terminal state. Results are fetchable.
    Ended,
    /// An unrecognized status; the SDK is older than the API.
    #[serde(other)]
    Other,
}

/// Per-status counts of entries within a batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RequestCounts {
    /// Entries still being processed.
    #[serde(default)]
    pub processing: u32,
    /// Entries that succeeded.
    #[serde(default)]
    pub succeeded: u32,
    /// Entries that errored.
    #[serde(default)]
    pub errored: u32,
    /// Entries canceled (via [`super::Batches::cancel`]).
    #[serde(default)]
    pub canceled: u32,
    /// Entries that expired before processing.
    #[serde(default)]
    pub expired: u32,
}

/// One per-entry result line from the JSONL results body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BatchResultItem {
    /// The `custom_id` echoed from the input.
    pub custom_id: String,
    /// Outcome for this entry.
    pub result: BatchResultPayload,
}

/// What happened to a batch entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BatchResultPayload {
    /// Entry completed successfully; `message` is the full response.
    Succeeded {
        /// The decoded [`Message`] response.
        message: Message,
    },
    /// Entry failed; `error` carries the API error payload.
    Errored {
        /// The decoded error payload.
        error: ApiErrorPayload,
    },
    /// Entry was canceled before it ran.
    Canceled,
    /// Entry's expiration deadline passed before it ran.
    Expired,
}

/// Query parameters for `GET /v1/messages/batches`.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct ListBatchesParams {
    /// Cursor for backward pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    /// Cursor for forward pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
    /// Page size (server-defaulted if omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl ListBatchesParams {
    /// Set the `after_id` cursor.
    #[must_use]
    pub fn after_id(mut self, id: impl Into<String>) -> Self {
        self.after_id = Some(id.into());
        self
    }

    /// Set the `before_id` cursor.
    #[must_use]
    pub fn before_id(mut self, id: impl Into<String>) -> Self {
        self.before_id = Some(id.into());
        self
    }

    /// Set the page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Confirmation returned by `DELETE /v1/messages/batches/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BatchDeleted {
    /// ID of the deleted batch.
    pub id: String,
    /// Wire `type`; always `"message_batch_deleted"`.
    #[serde(rename = "type", default)]
    pub kind: String,
}

/// Options controlling how [`super::Batches::wait_for`] polls.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WaitOptions {
    /// How often to call `GET /v1/messages/batches/{id}`.
    pub poll_interval: Duration,
    /// If set, give up after this duration.
    pub timeout: Option<Duration>,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(30),
            timeout: None,
        }
    }
}

impl WaitOptions {
    /// Set the polling interval.
    #[must_use]
    pub fn poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }

    /// Set an overall timeout. Without one, [`super::Batches::wait_for`]
    /// polls until the batch ends or the request itself errors.
    #[must_use]
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn message_batch_in_progress_round_trips() {
        let raw = json!({
            "id": "msgbatch_01ABC",
            "type": "message_batch",
            "processing_status": "in_progress",
            "request_counts": {
                "processing": 100,
                "succeeded": 0,
                "errored": 0,
                "canceled": 0,
                "expired": 0
            },
            "created_at": "2026-04-30T00:00:00Z",
            "expires_at": "2026-05-01T00:00:00Z",
            "ended_at": null,
            "archived_at": null,
            "cancel_initiated_at": null,
            "results_url": null
        });
        let parsed: MessageBatch = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.id, "msgbatch_01ABC");
        assert_eq!(parsed.kind, "message_batch");
        assert_eq!(parsed.processing_status, ProcessingStatus::InProgress);
        assert_eq!(parsed.request_counts.processing, 100);
        assert_eq!(parsed.ended_at, None);
    }

    #[test]
    fn message_batch_ended_includes_results_url() {
        let raw = json!({
            "id": "msgbatch_01XYZ",
            "type": "message_batch",
            "processing_status": "ended",
            "request_counts": {
                "processing": 0, "succeeded": 95, "errored": 3,
                "canceled": 0, "expired": 2
            },
            "created_at": "2026-04-30T00:00:00Z",
            "expires_at": "2026-05-01T00:00:00Z",
            "ended_at": "2026-04-30T01:00:00Z",
            "results_url": "https://example/results"
        });
        let parsed: MessageBatch = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.processing_status, ProcessingStatus::Ended);
        assert_eq!(parsed.request_counts.succeeded, 95);
        assert!(parsed.ended_at.is_some());
    }

    #[test]
    fn processing_status_unknown_falls_back_to_other() {
        let parsed: ProcessingStatus = serde_json::from_str("\"future_status\"").unwrap();
        assert_eq!(parsed, ProcessingStatus::Other);
    }

    #[test]
    fn batch_result_payload_succeeded_round_trips() {
        let raw = json!({
            "type": "succeeded",
            "message": {
                "id": "msg_X",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "hi"}],
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 1}
            }
        });
        let parsed: BatchResultPayload = serde_json::from_value(raw).unwrap();
        match parsed {
            BatchResultPayload::Succeeded { message } => {
                assert_eq!(message.id, "msg_X");
            }
            other => panic!("expected Succeeded, got {other:?}"),
        }
    }

    #[test]
    fn batch_result_payload_errored_round_trips() {
        let raw = json!({
            "type": "errored",
            "error": {"type": "rate_limit_error", "message": "slow down"}
        });
        let parsed: BatchResultPayload = serde_json::from_value(raw).unwrap();
        assert!(matches!(parsed, BatchResultPayload::Errored { .. }));
    }

    #[test]
    fn batch_result_payload_canceled_and_expired_round_trip() {
        let parsed: BatchResultPayload =
            serde_json::from_value(json!({"type": "canceled"})).unwrap();
        assert!(matches!(parsed, BatchResultPayload::Canceled));

        let parsed: BatchResultPayload =
            serde_json::from_value(json!({"type": "expired"})).unwrap();
        assert!(matches!(parsed, BatchResultPayload::Expired));
    }

    #[test]
    fn batch_result_item_round_trips() {
        let raw = json!({
            "custom_id": "req-42",
            "result": {"type": "canceled"}
        });
        let parsed: BatchResultItem = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.custom_id, "req-42");
        assert!(matches!(parsed.result, BatchResultPayload::Canceled));
    }
}
