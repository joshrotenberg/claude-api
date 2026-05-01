//! Error type, result alias, and wire-format error payload.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Crate-wide result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by this crate.
///
/// Variants tied to optional features (`async`/`sync` for [`Error::Network`],
/// `streaming` for [`Error::Stream`]) are conditionally compiled out when
/// those features are disabled. Use [`Error::is_retryable`] to decide
/// whether to retry; the [`crate::retry`] layer uses the same logic.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The Anthropic API returned an error response.
    #[error("API error ({status}): {message}")]
    #[non_exhaustive]
    Api {
        /// HTTP status code returned by the API.
        status: http::StatusCode,
        /// `request-id` header from the response, if present. Critical for
        /// support tickets.
        request_id: Option<String>,
        /// Decoded error category from the response body.
        kind: ApiErrorKind,
        /// Human-readable error message from the response body.
        message: String,
        /// `Retry-After` value parsed from the response, if present.
        retry_after: Option<Duration>,
    },
    /// Underlying HTTP transport failed (timeout, connection refused, DNS, etc.).
    #[cfg(any(feature = "async", feature = "sync"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "async", feature = "sync"))))]
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    /// Response body could not be parsed as JSON.
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),
    /// Streaming error (parse, connection lost, server-emitted error event).
    #[cfg(feature = "streaming")]
    #[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
    #[error("stream error: {0}")]
    Stream(#[from] StreamError),
    /// The [`crate::ClientBuilder`] was misconfigured.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    /// Local I/O failed (e.g. reading a file to upload).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// The agent loop runner reached its iteration limit without the model
    /// producing a non-`tool_use` stop reason.
    #[error("agent loop exceeded max iterations ({max})")]
    MaxIterationsExceeded {
        /// Configured iteration cap.
        max: u32,
    },
    /// The agent loop's configured cost budget was exceeded after a turn.
    /// `spent_usd` reflects the cumulative cost recorded on the conversation
    /// at the moment the budget check fired.
    #[error("agent loop exceeded cost budget: ${spent_usd:.4} > ${budget_usd:.4}")]
    CostBudgetExceeded {
        /// Configured ceiling.
        budget_usd: f64,
        /// Cumulative spend at the time of the check.
        spent_usd: f64,
    },
    /// A cancellation token signaled abort between iterations.
    #[error("agent loop cancelled")]
    Cancelled,
    /// A `ToolApprover` returned `ApprovalDecision::Stop`, ending the loop
    /// before the named tool could run.
    #[error("agent loop stopped by approval gate at tool '{tool_name}': {reason}")]
    ToolApprovalStopped {
        /// Name of the tool whose approval check returned `Stop`.
        tool_name: String,
        /// Reason supplied by the approver.
        reason: String,
    },
}

impl Error {
    /// Returns `true` if the error represents a transient failure worth retrying.
    ///
    /// Single source of truth used by both [`crate::retry::RetryPolicy`] and
    /// callers handling retries themselves.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Api { status, .. } => {
                matches!(
                    status.as_u16(),
                    408 | 425 | 429 | 500 | 502 | 503 | 504 | 529
                )
            }
            #[cfg(any(feature = "async", feature = "sync"))]
            Error::Network(e) => e.is_timeout() || e.is_connect(),
            #[cfg(feature = "streaming")]
            Error::Stream(_) => false,
            Error::Decode(_)
            | Error::InvalidConfig(_)
            | Error::Io(_)
            | Error::MaxIterationsExceeded { .. }
            | Error::CostBudgetExceeded { .. }
            | Error::Cancelled
            | Error::ToolApprovalStopped { .. } => false,
        }
    }

    /// `request-id` header from the API response, if this is an [`Error::Api`].
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Error::Api { request_id, .. } => request_id.as_deref(),
            _ => None,
        }
    }

    /// `Retry-After` value from the API response, if any.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Error::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// HTTP status code, if this is an [`Error::Api`].
    pub fn status(&self) -> Option<http::StatusCode> {
        match self {
            Error::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Build an [`Error::Api`] from the parts of an HTTP error response.
    ///
    /// `body` is the raw response body bytes; the function attempts to decode
    /// it as the standard `{"type": "error", "error": ApiErrorPayload}`
    /// envelope and falls back to a string-only payload if decoding fails.
    ///
    /// Used by the HTTP client; allowed-as-dead-code until task #8 lands.
    #[allow(dead_code)]
    pub(crate) fn from_response(
        status: http::StatusCode,
        request_id: Option<String>,
        retry_after_header: Option<&str>,
        body: &[u8],
    ) -> Error {
        let retry_after = retry_after_header.and_then(parse_retry_after);
        let payload = serde_json::from_slice::<ErrorEnvelope>(body).map_or_else(
            |_| ApiErrorPayload {
                kind: ApiErrorKind::ApiError,
                message: String::from_utf8_lossy(body).into_owned(),
            },
            |e| e.error,
        );
        Error::Api {
            status,
            request_id,
            kind: payload.kind,
            message: payload.message,
            retry_after,
        }
    }
}

/// Parse a `Retry-After` header value to a [`Duration`].
///
/// Supports the delta-seconds form only (e.g. `"120"`); HTTP-date form
/// returns `None`. Used by the HTTP client; allowed-as-dead-code until #8.
#[allow(dead_code)]
pub(crate) fn parse_retry_after(header: &str) -> Option<Duration> {
    header.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Internal wire envelope for HTTP error responses:
/// `{"type": "error", "error": ApiErrorPayload}`.
#[derive(Deserialize)]
#[allow(dead_code)]
struct ErrorEnvelope {
    error: ApiErrorPayload,
}

/// Wire-format error payload, as it appears inside an HTTP error response or
/// inside a streaming `error` event.
///
/// The wire shape is:
///
/// ```json
/// {"type": "overloaded_error", "message": "..."}
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ApiErrorPayload {
    /// Error category. Renamed from the wire `"type"` field for ergonomics.
    #[serde(rename = "type")]
    pub kind: ApiErrorKind,
    /// Human-readable error message.
    pub message: String,
}

/// Categories of errors the Anthropic API can return.
///
/// The wire form uses `snake_case` strings ending in `_error`
/// (e.g. `overloaded_error`). Unknown values deserialize to
/// [`ApiErrorKind::Other`] so a new error category from the server does not
/// break older SDK versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApiErrorKind {
    /// 400 -- request was malformed or violated API constraints.
    InvalidRequestError,
    /// 401 -- API key missing or invalid.
    AuthenticationError,
    /// 403 -- API key lacks permission for this resource.
    PermissionError,
    /// 404 -- resource does not exist.
    NotFoundError,
    /// 429 -- rate limit exceeded.
    RateLimitError,
    /// 500 -- internal server error.
    ApiError,
    /// 529 -- server is overloaded.
    OverloadedError,
    /// An unrecognized error category; the SDK is older than the API.
    #[serde(other)]
    Other,
}

/// Errors specific to the streaming layer.
///
/// Mid-stream failures cannot be retried safely (we'd silently drop content);
/// see [`Error::is_retryable`].
#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StreamError {
    /// Failed to parse a wire-level SSE event.
    #[error("stream parse error: {0}")]
    Parse(String),
    /// Connection dropped or other transport failure mid-stream.
    #[error("stream connection lost: {0}")]
    Connection(String),
    /// Server emitted a typed `error` event mid-stream.
    #[error("server emitted error event: {kind:?}: {message}")]
    Server {
        /// Error category from the event payload.
        kind: ApiErrorKind,
        /// Human-readable error message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn api_error_payload_round_trips() {
        let payload = ApiErrorPayload {
            kind: ApiErrorKind::OverloadedError,
            message: "server overloaded".into(),
        };
        let v = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            v,
            json!({"type": "overloaded_error", "message": "server overloaded"})
        );
        let parsed: ApiErrorPayload = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, payload);
    }

    #[test]
    fn api_error_kind_round_trips_known_variants() {
        for (variant, wire) in [
            (ApiErrorKind::InvalidRequestError, "invalid_request_error"),
            (ApiErrorKind::AuthenticationError, "authentication_error"),
            (ApiErrorKind::PermissionError, "permission_error"),
            (ApiErrorKind::NotFoundError, "not_found_error"),
            (ApiErrorKind::RateLimitError, "rate_limit_error"),
            (ApiErrorKind::ApiError, "api_error"),
            (ApiErrorKind::OverloadedError, "overloaded_error"),
        ] {
            let v = serde_json::to_value(variant).unwrap();
            assert_eq!(v, json!(wire));
            let parsed: ApiErrorKind = serde_json::from_value(v).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn api_error_kind_unknown_falls_to_other() {
        let parsed: ApiErrorKind = serde_json::from_str("\"future_error_type\"").unwrap();
        assert_eq!(parsed, ApiErrorKind::Other);
    }

    fn api_error(status: u16) -> Error {
        Error::Api {
            status: http::StatusCode::from_u16(status).unwrap(),
            request_id: None,
            kind: ApiErrorKind::ApiError,
            message: "x".into(),
            retry_after: None,
        }
    }

    #[test]
    fn is_retryable_for_transient_statuses() {
        for s in [408u16, 425, 429, 500, 502, 503, 504, 529] {
            assert!(api_error(s).is_retryable(), "{s} should retry");
        }
    }

    #[test]
    fn is_not_retryable_for_client_errors() {
        for s in [400u16, 401, 403, 404, 422] {
            assert!(!api_error(s).is_retryable(), "{s} should not retry");
        }
    }

    #[test]
    fn is_not_retryable_for_decode_invalidconfig_io() {
        let decode = Error::Decode(serde_json::from_str::<u32>("\"oops\"").unwrap_err());
        assert!(!decode.is_retryable());

        let cfg = Error::InvalidConfig("missing api key".into());
        assert!(!cfg.is_retryable());

        let io = Error::Io(std::io::Error::other("bad"));
        assert!(!io.is_retryable());
    }

    #[test]
    fn parse_retry_after_seconds() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after("  5 "), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
    }

    #[test]
    fn parse_retry_after_rejects_garbage() {
        assert_eq!(parse_retry_after("not a number"), None);
        // HTTP-date form is not supported in v0.1; we return None.
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert_eq!(parse_retry_after(""), None);
    }

    #[test]
    fn from_response_decodes_typed_error_envelope() {
        let body =
            br#"{"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}}"#;
        let err = Error::from_response(
            http::StatusCode::TOO_MANY_REQUESTS,
            Some("req_abc".into()),
            Some("12"),
            body,
        );
        match err {
            Error::Api {
                status,
                request_id,
                kind,
                message,
                retry_after,
            } => {
                assert_eq!(status, http::StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(request_id.as_deref(), Some("req_abc"));
                assert_eq!(kind, ApiErrorKind::RateLimitError);
                assert_eq!(message, "slow down");
                assert_eq!(retry_after, Some(Duration::from_secs(12)));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn from_response_falls_back_for_non_json_body() {
        let body = b"<html>oops</html>";
        let err = Error::from_response(http::StatusCode::BAD_GATEWAY, None, None, body);
        match err {
            Error::Api {
                status,
                kind,
                message,
                retry_after,
                ..
            } => {
                assert_eq!(status, http::StatusCode::BAD_GATEWAY);
                assert_eq!(kind, ApiErrorKind::ApiError); // fallback
                assert_eq!(message, "<html>oops</html>");
                assert_eq!(retry_after, None);
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn accessors_return_request_id_and_retry_after() {
        let err = Error::Api {
            status: http::StatusCode::INTERNAL_SERVER_ERROR,
            request_id: Some("rid".into()),
            kind: ApiErrorKind::ApiError,
            message: "boom".into(),
            retry_after: Some(Duration::from_secs(3)),
        };
        assert_eq!(err.request_id(), Some("rid"));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(3)));
        assert_eq!(err.status(), Some(http::StatusCode::INTERNAL_SERVER_ERROR));

        let cfg = Error::InvalidConfig("nope".into());
        assert_eq!(cfg.request_id(), None);
        assert_eq!(cfg.retry_after(), None);
        assert_eq!(cfg.status(), None);
    }

    #[test]
    fn display_impl_includes_status_and_message() {
        let err = api_error(503);
        let s = format!("{err}");
        assert!(s.contains("503"), "{s}");
        assert!(s.contains('x'), "{s}");
    }

    #[cfg(feature = "streaming")]
    #[test]
    fn stream_errors_are_not_retryable() {
        let err = Error::Stream(StreamError::Connection("dropped".into()));
        assert!(!err.is_retryable());
    }
}
