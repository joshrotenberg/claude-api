//! Live recording proxy for `claude-api-test`.
//!
//! [`Recorder`] runs an in-process HTTP forwarder on `127.0.0.1` that
//! captures every request a [`claude_api::Client`] sends through it,
//! tees the exchange to a JSONL cassette file, and returns the upstream
//! response unchanged. Pair with [`mount_cassette`](crate::mount_cassette)
//! and [`Cassette::from_path`](crate::Cassette::from_path) for replay.
//!
//! ```ignore
//! let recorder = Recorder::start(RecorderConfig {
//!     upstream: "https://api.anthropic.com".into(),
//!     cassette_path: "./cassette.jsonl".into(),
//!     ..Default::default()
//! }).await?;
//!
//! let client = claude_api::Client::builder()
//!     .api_key(env!("ANTHROPIC_API_KEY"))
//!     .base_url(recorder.url())
//!     .build()?;
//!
//! // ... drive the client; every request lands in cassette.jsonl ...
//!
//! recorder.shutdown().await?;
//! ```

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use http::HeaderMap;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

use crate::RecordedExchange;

/// Headers redacted from saved cassette entries by default.
///
/// Keeps API keys and bearer tokens out of files that get committed.
pub const DEFAULT_REDACT_HEADERS: &[&str] = &["x-api-key", "authorization"];

/// Configuration for [`Recorder::start`].
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// Upstream base URL the recorder forwards to. Trailing slashes are
    /// trimmed. Example: `"https://api.anthropic.com"`.
    pub upstream: String,
    /// Filesystem path for the JSONL cassette. The file is created if
    /// missing and appended to as exchanges complete.
    pub cassette_path: PathBuf,
    /// Header names (lowercase) whose values are dropped before being
    /// recorded to disk. Defaults to [`DEFAULT_REDACT_HEADERS`]. Body
    /// contents are *not* redacted -- callers should ensure prompts
    /// don't contain secrets they don't want in the cassette.
    pub redact_headers: Vec<String>,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            upstream: "https://api.anthropic.com".into(),
            cassette_path: PathBuf::from("./cassette.jsonl"),
            redact_headers: DEFAULT_REDACT_HEADERS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        }
    }
}

/// Live in-process recording proxy. Drop the value to begin shutdown,
/// or call [`Self::shutdown`] for an awaitable clean exit.
pub struct Recorder {
    url: String,
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Bind to `127.0.0.1:0`, spawn a forwarder task, and return a
    /// handle whose [`Self::url`] points at the proxy. The cassette
    /// file at `config.cassette_path` is opened in append mode (created
    /// if missing).
    pub async fn start(config: RecorderConfig) -> std::io::Result<Self> {
        let upstream = config.upstream.trim_end_matches('/').to_owned();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let local_addr = listener.local_addr()?;
        let url = format!("http://{local_addr}");

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.cassette_path)
            .await?;
        let writer = Arc::new(Mutex::new(file));

        // reqwest client used to forward requests upstream. Does NOT
        // share a connection pool with whatever the user's Client has;
        // that's fine for tests.
        let forwarder = reqwest::Client::builder()
            .build()
            .map_err(std::io::Error::other)?;

        let redact: Arc<Vec<String>> = Arc::new(
            config
                .redact_headers
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
        );

        let (tx, rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            tokio::pin!(rx);
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accept = listener.accept() => {
                        let Ok((stream, _peer)) = accept else { continue };
                        let upstream = upstream.clone();
                        let writer = Arc::clone(&writer);
                        let forwarder = forwarder.clone();
                        let redact = Arc::clone(&redact);
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);
                            let svc = service_fn(move |req| {
                                let upstream = upstream.clone();
                                let writer = Arc::clone(&writer);
                                let forwarder = forwarder.clone();
                                let redact = Arc::clone(&redact);
                                async move {
                                    handle_request(req, &upstream, &forwarder, writer, redact)
                                        .await
                                }
                            });
                            let _ = hyper::server::conn::http1::Builder::new()
                                .serve_connection(io, svc)
                                .await;
                        });
                    }
                }
            }
        });

        Ok(Self {
            url,
            shutdown: Some(tx),
            handle: Some(handle),
        })
    }

    /// Proxy URL the user should pass to
    /// `Client::builder().base_url(...)`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Signal the forwarder to stop accepting new connections, then
    /// await its task. Returns once the recorder has fully exited.
    pub async fn shutdown(mut self) -> std::io::Result<()> {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        Ok(())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle_request(
    req: Request<Incoming>,
    upstream: &str,
    forwarder: &reqwest::Client,
    writer: Arc<Mutex<tokio::fs::File>>,
    redact: Arc<Vec<String>>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map_or_else(|| req.uri().path().to_owned(), ToString::to_string);
    let path_only = req.uri().path().to_owned();
    let headers = req.headers().clone();

    let body_bytes = match req.into_body().collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => {
            return Ok(error_response(
                http::StatusCode::BAD_GATEWAY,
                "recorder: failed to read request body",
            ));
        }
    };

    // Forward upstream.
    let url = format!("{upstream}{path_and_query}");
    let mut fwd = forwarder.request(method.clone(), &url);
    for (name, value) in &headers {
        // Hop-by-hop and host headers are unsafe to forward verbatim.
        if matches!(name.as_str(), "host" | "content-length") {
            continue;
        }
        fwd = fwd.header(name, value);
    }
    if !body_bytes.is_empty() {
        fwd = fwd.body(body_bytes.to_vec());
    }
    let upstream_resp = match fwd.send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(error_response(
                http::StatusCode::BAD_GATEWAY,
                &format!("recorder: upstream request failed: {e}"),
            ));
        }
    };
    let status = upstream_resp.status();
    let upstream_headers = upstream_resp.headers().clone();
    let resp_bytes = upstream_resp.bytes().await.unwrap_or_default();

    // Capture the exchange.
    let exchange = build_exchange(
        method.as_str(),
        &path_only,
        status.as_u16(),
        &body_bytes,
        &upstream_headers,
        &resp_bytes,
        &redact,
    );
    // Suppress unused_variables warning for `headers` -- we kept it
    // bound for symmetry with the response side, and to leave a hook
    // for redaction-policy expansion (e.g. recording the Authorization
    // *presence* without its value).
    let _ = &headers;
    if let Ok(line) = serde_json::to_string(&exchange) {
        let mut guard = writer.lock().await;
        let _ = guard.write_all(line.as_bytes()).await;
        let _ = guard.write_all(b"\n").await;
        let _ = guard.flush().await;
    }

    // Build the response we send back to the client.
    let mut builder = Response::builder().status(status);
    for (name, value) in &upstream_headers {
        builder = builder.header(name, value);
    }
    let response = builder
        .body(Full::new(resp_bytes))
        .unwrap_or_else(|_| error_response(http::StatusCode::BAD_GATEWAY, "recorder: build error"));
    Ok(response)
}

fn build_exchange(
    method: &str,
    path: &str,
    status: u16,
    request_body: &[u8],
    response_headers: &HeaderMap,
    response_body: &[u8],
    redact: &[String],
) -> RecordedExchange {
    // Decode bodies as JSON when possible; bare-bytes payloads (e.g.
    // multipart uploads) fall back to a base64-ish stand-in -- but in
    // practice the API surface is JSON, and this recorder is scoped to
    // claude-api whose endpoints are all JSON or SSE.
    let request_value = if request_body.is_empty() {
        None
    } else {
        Some(
            serde_json::from_slice::<serde_json::Value>(request_body).unwrap_or_else(|_| {
                serde_json::Value::String(format!("<{} bytes>", request_body.len()))
            }),
        )
    };
    let response_value = serde_json::from_slice::<serde_json::Value>(response_body)
        .unwrap_or_else(|_| serde_json::Value::String(format!("<{} bytes>", response_body.len())));

    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, value) in response_headers {
        let name_lc = name.as_str().to_lowercase();
        if redact.iter().any(|r| r == &name_lc) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            headers.push((name_lc, v.to_owned()));
        }
    }

    RecordedExchange {
        method: method.to_owned(),
        path: path.to_owned(),
        status,
        request: request_value,
        response: response_value,
        headers,
    }
}

fn error_response(status: http::StatusCode, message: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(format!(
            r#"{{"type":"error","error":{{"type":"recorder_error","message":{message:?}}}}}"#
        ))))
        .expect("static response is well-formed")
}
