//! Cassette-based replay for `claude-api` integration tests.
//!
//! Records of `request → response` exchanges are stored as JSONL on disk
//! and served via [`wiremock`]. Tests point a `claude_api::Client` at the
//! wiremock server's URL and exercise the live code paths against the
//! canned responses -- no network calls, deterministic, reviewable in
//! version control.
//!
//! # Format
//!
//! Each line of a cassette file is one [`RecordedExchange`]:
//!
//! ```jsonl
//! {"method":"POST","path":"/v1/messages","status":200,"request":{...},"response":{...}}
//! {"method":"GET","path":"/v1/models","status":200,"request":null,"response":{...}}
//! ```
//!
//! `request` is the optional decoded JSON body; `response` is the
//! response body. The matcher pairs a live request with the *first*
//! cassette entry whose `(method, path)` and `request` match. Use
//! [`Cassette::skip_request_match`] to disable body matching when you
//! only care about the URL.
//!
//! # Quick start
//!
//! ```ignore
//! use claude_api::{Client, messages::CreateMessageRequest, types::ModelId};
//! use claude_api_test::{mount_cassette, Cassette};
//! use wiremock::MockServer;
//!
//! #[tokio::test]
//! async fn replay_messages_create() {
//!     let cassette = Cassette::from_path("tests/cassettes/messages_create.jsonl")
//!         .await
//!         .unwrap();
//!     let server = MockServer::start().await;
//!     mount_cassette(&server, &cassette).await;
//!
//!     let client = Client::builder()
//!         .api_key("sk-ant-test")
//!         .base_url(server.uri())
//!         .build()
//!         .unwrap();
//!     let req = CreateMessageRequest::builder()
//!         .model(ModelId::SONNET_4_6)
//!         .max_tokens(64)
//!         .user("hi")
//!         .build()
//!         .unwrap();
//!     let resp = client.messages().create(req).await.unwrap();
//!     assert_eq!(resp.id, "msg_replay");
//! }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]

use std::path::Path;

use serde::{Deserialize, Serialize};

pub mod recorder;
pub use recorder::{DEFAULT_REDACT_HEADERS, Recorder, RecorderConfig};

/// One recorded HTTP exchange. Preserved on disk as one JSONL line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RecordedExchange {
    /// HTTP method (`GET`, `POST`, etc.).
    pub method: String,
    /// URL path (e.g. `/v1/messages`).
    pub path: String,
    /// HTTP status code returned.
    pub status: u16,
    /// Decoded JSON request body, or `None` if the original request had
    /// no body. Used as a matching constraint in
    /// [`mount_cassette`] unless `skip_request_match` is set.
    #[serde(default)]
    pub request: Option<serde_json::Value>,
    /// Decoded JSON response body. Stored as `Value` so the cassette
    /// stays human-readable and diffable.
    pub response: serde_json::Value,
    /// Optional response headers to set when serving (e.g.
    /// `request-id`, `retry-after`). Defaults to none.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

impl RecordedExchange {
    /// Build a `RecordedExchange` with no request-body match constraint
    /// and no extra response headers. Use the field setters to refine.
    #[must_use]
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        status: u16,
        response: serde_json::Value,
    ) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            status,
            request: None,
            response,
            headers: Vec::new(),
        }
    }

    /// Add a request-body match constraint.
    #[must_use]
    pub fn with_request(mut self, body: serde_json::Value) -> Self {
        self.request = Some(body);
        self
    }

    /// Add a single response header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// A collection of [`RecordedExchange`]s, typically loaded from a JSONL
/// file. Mount on a [`wiremock::MockServer`] via [`mount_cassette`].
#[derive(Debug, Clone, Default)]
pub struct Cassette {
    exchanges: Vec<RecordedExchange>,
    skip_request_match: bool,
}

impl Cassette {
    /// Build an empty cassette.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from an in-memory list of exchanges. Useful for tests that
    /// inline their fixtures.
    #[must_use]
    pub fn from_exchanges(exchanges: Vec<RecordedExchange>) -> Self {
        Self {
            exchanges,
            skip_request_match: false,
        }
    }

    /// Async-load a cassette from a JSONL file at `path`. Lines that are
    /// blank or start with `#` are skipped (so cassettes can carry
    /// human-readable comments).
    pub async fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let text = tokio::fs::read_to_string(path).await?;
        Self::parse_jsonl(&text).map_err(std::io::Error::other)
    }

    /// Synchronous version of [`Self::from_path`]. Convenient when you
    /// don't have a runtime in scope.
    pub fn from_path_sync(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse_jsonl(&text).map_err(std::io::Error::other)
    }

    /// Parse a JSONL string into a cassette. Renamed from `from_str`
    /// to avoid clashing with `std::str::FromStr::from_str`.
    pub fn parse_jsonl(jsonl: &str) -> serde_json::Result<Self> {
        let mut exchanges = Vec::new();
        for (line_no, line) in jsonl.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let exchange: RecordedExchange = serde_json::from_str(trimmed).map_err(|e| {
                let msg = format!("cassette parse failed at line {}: {}", line_no + 1, e);
                serde::de::Error::custom(msg)
            })?;
            exchanges.push(exchange);
        }
        Ok(Self {
            exchanges,
            skip_request_match: false,
        })
    }

    /// Append an exchange.
    pub fn push(&mut self, exchange: RecordedExchange) -> &mut Self {
        self.exchanges.push(exchange);
        self
    }

    /// Disable request-body matching. The wiremock matcher will pair
    /// requests by `(method, path)` only. Useful when the request body
    /// includes nondeterministic fields (timestamps, request IDs).
    #[must_use]
    pub fn skip_request_match(mut self) -> Self {
        self.skip_request_match = true;
        self
    }

    /// Borrow the underlying exchange list.
    #[must_use]
    pub fn exchanges(&self) -> &[RecordedExchange] {
        &self.exchanges
    }

    /// Total number of exchanges in this cassette.
    #[must_use]
    pub fn len(&self) -> usize {
        self.exchanges.len()
    }

    /// `true` if the cassette has no exchanges.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exchanges.is_empty()
    }

    /// Serialize back to JSONL. Round-trips with [`Self::parse_jsonl`].
    pub fn to_jsonl(&self) -> serde_json::Result<String> {
        let mut out = String::new();
        for ex in &self.exchanges {
            out.push_str(&serde_json::to_string(ex)?);
            out.push('\n');
        }
        Ok(out)
    }
}

/// Mount every exchange in `cassette` on `server`. Each exchange becomes
/// one [`wiremock::Mock`] that matches `(method, path)` (and the request
/// body, unless [`Cassette::skip_request_match`] was set).
///
/// Mocks are mounted in cassette order. wiremock's first-match semantics
/// mean that for two exchanges with the same `(method, path)`, the
/// earlier one wins -- match by request body to disambiguate.
pub async fn mount_cassette(server: &wiremock::MockServer, cassette: &Cassette) {
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, ResponseTemplate};

    for ex in &cassette.exchanges {
        let mut response = ResponseTemplate::new(ex.status).set_body_json(ex.response.clone());
        for (k, v) in &ex.headers {
            response = response.insert_header(k.as_str(), v.as_str());
        }

        let mock_builder = Mock::given(method(ex.method.as_str())).and(path(ex.path.as_str()));
        let mock = match (&ex.request, cassette.skip_request_match) {
            (Some(body), false) => mock_builder.and(body_json(body)).respond_with(response),
            _ => mock_builder.respond_with(response),
        };
        mock.mount(server).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_jsonl_round_trips() {
        let jsonl = r#"
# leading comment, ignored
{"method":"POST","path":"/v1/messages","status":200,"request":{"model":"x"},"response":{"id":"msg_1"}}
{"method":"GET","path":"/v1/models","status":200,"request":null,"response":{"data":[]}}
"#;
        let c = Cassette::parse_jsonl(jsonl).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c.exchanges()[0].method, "POST");
        assert_eq!(c.exchanges()[1].path, "/v1/models");

        let serialized = c.to_jsonl().unwrap();
        let again = Cassette::parse_jsonl(&serialized).unwrap();
        assert_eq!(again.len(), 2);
    }

    #[test]
    fn empty_cassette_is_empty() {
        let c = Cassette::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn cassette_parse_error_includes_line_number() {
        let jsonl = "not-json\n";
        let err = Cassette::parse_jsonl(jsonl).unwrap_err();
        assert!(format!("{err}").contains("line 1"));
    }

    #[test]
    fn skip_request_match_flag_is_set() {
        let c = Cassette::new().skip_request_match();
        assert!(c.skip_request_match);
    }

    #[test]
    fn from_exchanges_constructs_directly() {
        let ex = RecordedExchange {
            method: "POST".into(),
            path: "/v1/x".into(),
            status: 200,
            request: Some(json!({"k": 1})),
            response: json!({"ok": true}),
            headers: vec![("request-id".into(), "req_1".into())],
        };
        let c = Cassette::from_exchanges(vec![ex]);
        assert_eq!(c.len(), 1);
    }
}
