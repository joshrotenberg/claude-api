//! HTTP client and builder.
//!
//! [`Client`] is the entry point to the SDK. It is cheap to [`Clone`] (an
//! `Arc<Inner>` under the hood) and `Send + Sync`, so a single instance can
//! be shared across tasks.

#![cfg(feature = "async")]

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::auth::{ApiKey, ApiKeySigner, RequestSigner};
use crate::error::{Error, Result};
use crate::retry::RetryPolicy;

/// HTTP client for the Anthropic API.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    base_url: String,
    http: reqwest::Client,
    user_agent: String,
    betas: Vec<String>,
    retry: RetryPolicy,
    signer: Arc<dyn RequestSigner>,
}

impl Client {
    /// Construct a [`Client`] with default settings and the given API key.
    ///
    /// # Panics
    ///
    /// Panics if reqwest fails to build its default HTTP client (extremely
    /// unusual; would indicate a broken TLS stack). Use [`Client::builder`]
    /// for a fallible alternative.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::builder()
            .api_key(api_key)
            .build()
            .expect("default builder should succeed when an api key is provided")
    }

    /// Begin configuring a [`Client`].
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Namespace handle for the Messages API.
    pub fn messages(&self) -> crate::messages::Messages<'_> {
        crate::messages::Messages::new(self)
    }

    /// Namespace handle for the Models API.
    pub fn models(&self) -> crate::models::Models<'_> {
        crate::models::Models::new(self)
    }

    /// Namespace handle for the Batches API.
    pub fn batches(&self) -> crate::batches::Batches<'_> {
        crate::batches::Batches::new(self)
    }

    /// Namespace handle for the Files API (beta).
    pub fn files(&self) -> crate::files::Files<'_> {
        crate::files::Files::new(self)
    }

    /// Namespace handle for the Managed Agents API (preview).
    ///
    /// Gated on the `managed-agents-preview` feature.
    #[cfg(feature = "managed-agents-preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "managed-agents-preview")))]
    pub fn managed_agents(&self) -> crate::managed_agents::ManagedAgents<'_> {
        crate::managed_agents::ManagedAgents::new(self)
    }

    /// Namespace handle for the Admin API. Requires an admin API key.
    ///
    /// Gated on the `admin` feature.
    #[cfg(feature = "admin")]
    #[cfg_attr(docsrs, doc(cfg(feature = "admin")))]
    pub fn admin(&self) -> crate::admin::Admin<'_> {
        crate::admin::Admin::new(self)
    }

    /// Namespace handle for the Skills API (beta).
    ///
    /// Gated on the `skills` feature.
    #[cfg(feature = "skills")]
    #[cfg_attr(docsrs, doc(cfg(feature = "skills")))]
    pub fn skills(&self) -> crate::skills::Skills<'_> {
        crate::skills::Skills::new(self)
    }

    /// Namespace handle for the User Profiles API (beta).
    ///
    /// Gated on the `user-profiles` feature.
    #[cfg(feature = "user-profiles")]
    #[cfg_attr(docsrs, doc(cfg(feature = "user-profiles")))]
    pub fn user_profiles(&self) -> crate::user_profiles::UserProfiles<'_> {
        crate::user_profiles::UserProfiles::new(self)
    }

    /// Build a [`reqwest::RequestBuilder`] preloaded with the version
    /// and user-agent headers. Auth headers are added later by the
    /// configured [`RequestSigner`](crate::auth::RequestSigner). Endpoints
    /// add their body and any per-request beta headers, then call
    /// [`Self::execute`].
    pub(crate) fn request_builder(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.inner.base_url, path);
        self.inner
            .http
            .request(method, url)
            .header("anthropic-version", crate::ANTHROPIC_VERSION)
            .header(reqwest::header::USER_AGENT, &self.inner.user_agent)
    }

    /// Send a prepared request, merge in beta headers, and decode the response.
    ///
    /// Errors from the API (any non-2xx status) are mapped to [`Error::Api`]
    /// with `request-id` and `Retry-After` populated when the server sent
    /// them. The retry loop ([`Self::execute_with_retry`]) wraps this method.
    pub(crate) async fn execute<R: DeserializeOwned>(
        &self,
        mut builder: reqwest::RequestBuilder,
        per_request_betas: &[&str],
    ) -> Result<R> {
        if let Some(joined) = merge_betas(&self.inner.betas, per_request_betas) {
            builder = builder.header("anthropic-beta", joined);
        }

        let mut request = builder.build()?;
        self.inner
            .signer
            .sign(&mut request)
            .map_err(Error::Signing)?;
        let response = self.inner.http.execute(request).await?;
        let status = response.status();
        let request_id = response
            .headers()
            .get("request-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let retry_after_header = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let bytes = response.bytes().await?;

        if !status.is_success() {
            tracing::warn!(
                status = status.as_u16(),
                request_id = ?request_id,
                "claude-api: error response"
            );
            return Err(Error::from_response(
                http::StatusCode::from_u16(status.as_u16())
                    .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR),
                request_id,
                retry_after_header.as_deref(),
                &bytes,
            ));
        }

        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Send a request with retries.
    ///
    /// `make_request` is called once per attempt to produce a fresh
    /// [`reqwest::RequestBuilder`]. Retries are gated by
    /// [`Error::is_retryable`] and spaced according to
    /// [`RetryPolicy::compute_backoff`]. Streaming endpoints intentionally do
    /// *not* go through this path -- a mid-stream retry would silently drop
    /// content.
    pub(crate) async fn execute_with_retry<R, F>(
        &self,
        mut make_request: F,
        per_request_betas: &[&str],
    ) -> Result<R>
    where
        R: DeserializeOwned,
        F: FnMut() -> reqwest::RequestBuilder,
    {
        let policy = &self.inner.retry;
        let mut attempt: u32 = 1;
        loop {
            let builder = make_request();
            match self.execute(builder, per_request_betas).await {
                Ok(r) => return Ok(r),
                Err(e) => {
                    if !e.is_retryable() || attempt >= policy.max_attempts {
                        return Err(e);
                    }
                    let backoff = policy.compute_backoff(attempt, e.retry_after());
                    tracing::warn!(
                        attempt,
                        retry_in_ms = u64::try_from(backoff.as_millis()).unwrap_or(u64::MAX),
                        request_id = ?e.request_id(),
                        status = ?e.status().map(|s| s.as_u16()),
                        "claude-api: retrying after error"
                    );
                    tokio::time::sleep(backoff).await;
                    attempt += 1;
                }
            }
        }
    }

    /// Send a request expected to return a streaming body.
    ///
    /// Returns the raw [`reqwest::Response`] on success so the caller can
    /// wrap its body in an SSE parser, JSONL parser, or other line-oriented
    /// reader. Non-2xx responses are mapped to [`Error::Api`] (with
    /// `request-id` and `Retry-After`) just like [`Self::execute`]; the
    /// body is consumed in that case.
    ///
    /// Streaming is *not* retried -- once the server starts emitting events,
    /// retrying mid-stream would silently drop content.
    pub(crate) async fn execute_streaming(
        &self,
        mut builder: reqwest::RequestBuilder,
        per_request_betas: &[&str],
    ) -> Result<reqwest::Response> {
        if let Some(joined) = merge_betas(&self.inner.betas, per_request_betas) {
            builder = builder.header("anthropic-beta", joined);
        }

        let mut request = builder.build()?;
        self.inner
            .signer
            .sign(&mut request)
            .map_err(Error::Signing)?;
        let response = self.inner.http.execute(request).await?;
        let status = response.status();

        if !status.is_success() {
            let request_id = response
                .headers()
                .get("request-id")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let retry_after_header = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let bytes = response.bytes().await?;
            tracing::warn!(
                status = status.as_u16(),
                request_id = ?request_id,
                "claude-api: streaming connect failed"
            );
            return Err(Error::from_response(
                http::StatusCode::from_u16(status.as_u16())
                    .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR),
                request_id,
                retry_after_header.as_deref(),
                &bytes,
            ));
        }

        Ok(response)
    }

    #[cfg(test)]
    pub(crate) fn betas(&self) -> &[String] {
        &self.inner.betas
    }

    /// Materialize a request without sending it, for use by namespace-level
    /// `dry_run` helpers. Mirrors the header logic in
    /// [`Self::execute`]/[`Self::execute_streaming`] so the rendered
    /// preview matches what would actually be transmitted.
    pub(crate) fn render_dry_run(
        &self,
        mut builder: reqwest::RequestBuilder,
        per_request_betas: &[&str],
    ) -> Result<crate::dry_run::DryRun> {
        if let Some(joined) = merge_betas(&self.inner.betas, per_request_betas) {
            builder = builder.header("anthropic-beta", joined);
        }
        let mut req = builder.build()?;
        // Run the signer through dry_run too so the rendered preview
        // matches the wire bytes the live client would actually send.
        self.inner.signer.sign(&mut req).map_err(Error::Signing)?;
        let method = req.method().clone();
        let url = req.url().to_string();
        let mut headers = http::HeaderMap::new();
        for (name, value) in req.headers() {
            // Convert reqwest::header::HeaderName/Value (re-exports of http
            // types) into the http crate's owned types.
            if let (Ok(name), Ok(value)) = (
                http::HeaderName::from_bytes(name.as_ref()),
                http::HeaderValue::from_bytes(value.as_bytes()),
            ) {
                headers.append(name, value);
            }
        }
        let body = if let Some(body) = req.body() {
            if let Some(bytes) = body.as_bytes() {
                serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        } else {
            serde_json::Value::Null
        };
        Ok(crate::dry_run::DryRun {
            method,
            url,
            headers,
            body,
        })
    }
}

/// Merge client-level and per-request beta values into a single
/// comma-joined header value.
///
/// Order is preserved: client-level betas first, in insertion order, then
/// per-request betas. Empty or whitespace-only entries are dropped, and
/// each entry is trimmed. Returns `None` if no entries remain.
fn merge_betas(client_betas: &[String], per_request_betas: &[&str]) -> Option<String> {
    let trimmed: Vec<&str> = client_betas
        .iter()
        .map(String::as_str)
        .chain(per_request_betas.iter().copied())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.join(","))
    }
}

/// Builder for [`Client`].
#[derive(Default)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    user_agent: Option<String>,
    timeout: Option<Duration>,
    betas: Vec<String>,
    retry: Option<RetryPolicy>,
    http: Option<reqwest::Client>,
    signer: Option<Arc<dyn RequestSigner>>,
}

impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("user_agent", &self.user_agent)
            .field("timeout", &self.timeout)
            .field("betas", &self.betas)
            .field("retry", &self.retry)
            .field("http", &self.http.is_some())
            .field("signer", &self.signer.as_ref().map(|s| format!("{s:?}")))
            .finish()
    }
}

impl ClientBuilder {
    /// API key; required.
    #[must_use]
    pub fn api_key(mut self, k: impl Into<String>) -> Self {
        self.api_key = Some(k.into());
        self
    }

    /// Override the base URL. Useful for proxies and `wiremock`-based tests.
    /// Defaults to `https://api.anthropic.com`.
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Append an `anthropic-beta` value. May be called multiple times; values
    /// are comma-joined per Anthropic convention.
    #[must_use]
    pub fn beta(mut self, header_value: impl Into<String>) -> Self {
        self.betas.push(header_value.into());
        self
    }

    /// Per-request timeout applied to the underlying reqwest client.
    /// Ignored if a custom HTTP client is supplied via [`Self::http_client`].
    #[must_use]
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    /// Override the default retry policy.
    #[must_use]
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = Some(policy);
        self
    }

    /// Supply your own [`reqwest::Client`]. Lets callers reuse a connection
    /// pool, install custom middleware, or configure proxy / TLS settings
    /// outside the SDK.
    #[must_use]
    pub fn http_client(mut self, c: reqwest::Client) -> Self {
        self.http = Some(c);
        self
    }

    /// Override the `User-Agent` header. Defaults to `claude-api-rs/<version>`.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Install a custom [`RequestSigner`]. If unset, the builder
    /// defaults to [`ApiKeySigner`] from the configured `api_key`.
    /// Setting both is allowed: the explicit signer takes precedence
    /// (useful for tests that want a no-op signer with an unused
    /// placeholder key).
    #[must_use]
    pub fn signer(mut self, signer: Arc<dyn RequestSigner>) -> Self {
        self.signer = Some(signer);
        self
    }

    /// Construct the [`Client`]. Returns [`Error::InvalidConfig`] if
    /// neither an `api_key` nor a custom `signer` was provided.
    pub fn build(self) -> Result<Client> {
        let signer: Arc<dyn RequestSigner> = if let Some(s) = self.signer {
            s
        } else if let Some(key) = self.api_key {
            Arc::new(ApiKeySigner::new(ApiKey::new(key)))
        } else {
            return Err(Error::InvalidConfig(
                "either api_key or signer must be configured".into(),
            ));
        };

        let http = if let Some(c) = self.http {
            c
        } else {
            let mut builder = reqwest::Client::builder();
            if let Some(t) = self.timeout {
                builder = builder.timeout(t);
            }
            builder.build()?
        };

        let inner = Inner {
            base_url: self
                .base_url
                .unwrap_or_else(|| crate::DEFAULT_BASE_URL.to_owned()),
            http,
            user_agent: self
                .user_agent
                .unwrap_or_else(|| crate::USER_AGENT.to_owned()),
            betas: self.betas,
            retry: self.retry.unwrap_or_default(),
            signer,
        };

        Ok(Client {
            inner: Arc::new(inner),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::json;
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Deserialize, Debug, PartialEq)]
    struct Pong {
        ok: bool,
    }

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test-key")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[test]
    fn build_requires_api_key() {
        let err = Client::builder().build().unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)), "{err:?}");
    }

    #[cfg(feature = "bedrock")]
    #[tokio::test]
    async fn bedrock_signer_replaces_x_api_key_with_sigv4_headers() {
        use crate::bedrock::{BedrockCredentials, BedrockSigner};
        use wiremock::matchers::header_regex;
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            // sigv4 always emits an Authorization header beginning with the algorithm prefix.
            .and(header_regex("authorization", "^AWS4-HMAC-SHA256 "))
            // x-amz-date is the canonical timestamp header.
            .and(header_exists("x-amz-date"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let signer = std::sync::Arc::new(BedrockSigner::new(
            BedrockCredentials::new("AKIDEXAMPLE", "secret"),
            "us-east-1",
        ));
        let client = Client::builder()
            .api_key("sk-ant-unused")
            .base_url(mock.uri())
            .signer(signer)
            .build()
            .unwrap();

        let _: Pong = client
            .execute(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap();

        // Wiremock would 404 if the signer hadn't run; explicit
        // negative check on the live captured request:
        let received = &mock.received_requests().await.unwrap()[0];
        assert!(
            received.headers.get("x-api-key").is_none(),
            "x-api-key should not be set when a custom signer is installed",
        );
    }

    #[test]
    fn client_is_cheap_to_clone() {
        let c1 = Client::new("sk-ant-x");
        let c2 = c1.clone();
        // Both clones point at the same Arc<Inner>.
        assert!(Arc::ptr_eq(&c1.inner, &c2.inner));
    }

    #[tokio::test]
    async fn execute_sets_default_headers_and_decodes_response() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .and(header("x-api-key", "sk-ant-test-key"))
            .and(header("anthropic-version", crate::ANTHROPIC_VERSION))
            .and(header_exists("user-agent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let resp: Pong = client
            .execute(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(resp, Pong { ok: true });
    }

    #[tokio::test]
    async fn beta_headers_from_builder_are_applied_and_comma_joined() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .and(header_exists("anthropic-beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .beta("feat-a")
            .beta("feat-b")
            .build()
            .unwrap();

        let _: Pong = client
            .execute(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap();

        let req = &mock.received_requests().await.unwrap()[0];
        let beta = req.headers.get("anthropic-beta").unwrap().to_str().unwrap();
        assert_eq!(beta, "feat-a,feat-b");
    }

    #[tokio::test]
    async fn per_request_betas_merge_with_builder_betas() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .and(header_exists("anthropic-beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .beta("client-level")
            .build()
            .unwrap();

        let _: Pong = client
            .execute(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &["per-req"],
            )
            .await
            .unwrap();

        let req = &mock.received_requests().await.unwrap()[0];
        let beta = req.headers.get("anthropic-beta").unwrap().to_str().unwrap();
        assert_eq!(beta, "client-level,per-req");
    }

    #[tokio::test]
    async fn no_beta_header_when_none_configured() {
        let mock = MockServer::start().await;
        // We can't easily assert "header NOT present" with wiremock matchers,
        // but if the request fails to match our (no-beta) mock, the call would
        // 404 and the assert below would fire.
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .expect(1)
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let _: Pong = client
            .execute(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn error_response_maps_to_api_error_with_request_id_and_retry_after() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("request-id", "req_abc123")
                    .insert_header("retry-after", "8")
                    .set_body_json(json!({
                        "type": "error",
                        "error": {
                            "type": "rate_limit_error",
                            "message": "slow down please"
                        }
                    })),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let err = client
            .execute::<Pong>(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap_err();

        match err {
            Error::Api {
                status,
                request_id,
                kind,
                message,
                retry_after,
            } => {
                assert_eq!(status, http::StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(request_id.as_deref(), Some("req_abc123"));
                assert_eq!(kind, crate::error::ApiErrorKind::RateLimitError);
                assert_eq!(message, "slow down please");
                assert_eq!(retry_after, Some(Duration::from_secs(8)));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_json_error_body_falls_back_to_api_error() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>"))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let err = client
            .execute::<Pong>(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap_err();

        match err {
            Error::Api {
                status,
                message,
                kind,
                ..
            } => {
                assert_eq!(status, http::StatusCode::BAD_GATEWAY);
                assert_eq!(kind, crate::error::ApiErrorKind::ApiError);
                assert!(message.contains("bad gateway"), "{message}");
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_success_body_maps_to_decode_error() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let err = client
            .execute::<Pong>(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Decode(_)), "{err:?}");
    }

    #[tokio::test]
    async fn custom_user_agent_overrides_default() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .and(header("user-agent", "my-app/1.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .user_agent("my-app/1.0")
            .build()
            .unwrap();

        let _: Pong = client
            .execute(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap();
    }

    fn fast_retry_policy() -> crate::retry::RetryPolicy {
        crate::retry::RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(5),
            jitter: crate::retry::Jitter::None,
            respect_retry_after: false,
        }
    }

    #[tokio::test]
    async fn execute_with_retry_succeeds_after_transient_failure() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .retry(fast_retry_policy())
            .build()
            .unwrap();

        let resp: Pong = client
            .execute_with_retry(
                || client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap();
        assert!(resp.ok);
        // Two requests total: one 503 retry + one success.
        assert_eq!(mock.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn execute_with_retry_gives_up_after_max_attempts() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .retry(fast_retry_policy())
            .build()
            .unwrap();

        let err = client
            .execute_with_retry::<Pong, _>(
                || client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap_err();
        assert_eq!(err.status(), Some(http::StatusCode::SERVICE_UNAVAILABLE));
        // max_attempts = 3 -> 3 total requests.
        assert_eq!(mock.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn execute_with_retry_does_not_retry_non_retryable_errors() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "type": "error",
                "error": {"type": "invalid_request_error", "message": "bad input"}
            })))
            .expect(1)
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .retry(fast_retry_policy())
            .build()
            .unwrap();

        let err = client
            .execute_with_retry::<Pong, _>(
                || client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap_err();
        assert_eq!(err.status(), Some(http::StatusCode::BAD_REQUEST));
        assert_eq!(mock.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn execute_with_retry_honors_retry_after_header() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "0")
                    .set_body_json(json!({
                        "type": "error",
                        "error": {"type": "rate_limit_error", "message": "slow down"}
                    })),
            )
            .up_to_n_times(1)
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .retry(crate::retry::RetryPolicy {
                respect_retry_after: true,
                ..fast_retry_policy()
            })
            .build()
            .unwrap();

        let resp: Pong = client
            .execute_with_retry(
                || client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &[],
            )
            .await
            .unwrap();
        assert!(resp.ok);
    }

    #[test]
    fn builder_collects_betas_in_order() {
        let client = Client::builder()
            .api_key("sk-ant-x")
            .beta("a")
            .beta("b")
            .beta("c")
            .build()
            .unwrap();
        assert_eq!(
            client.betas(),
            &["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
    }

    #[test]
    fn merge_betas_returns_none_when_all_inputs_empty_or_whitespace() {
        assert_eq!(merge_betas(&[], &[]), None);
        assert_eq!(
            merge_betas(&[String::new(), "   ".into()], &["", "  "]),
            None
        );
    }

    #[test]
    fn merge_betas_filters_empties_and_trims() {
        let client_betas = vec!["  feat-a  ".to_owned(), String::new(), "feat-b".to_owned()];
        let per_request = ["", "feat-c\n", "  "];
        assert_eq!(
            merge_betas(&client_betas, &per_request).as_deref(),
            Some("feat-a,feat-b,feat-c")
        );
    }

    #[test]
    fn merge_betas_preserves_order_client_then_per_request() {
        assert_eq!(
            merge_betas(&["x".into(), "y".into()], &["z", "w"]).as_deref(),
            Some("x,y,z,w")
        );
    }

    #[test]
    fn merge_betas_keeps_duplicates_intact() {
        // Dedup is intentionally NOT performed; users manage their own set.
        assert_eq!(
            merge_betas(&["foo".into()], &["foo"]).as_deref(),
            Some("foo,foo")
        );
    }

    #[tokio::test]
    async fn beta_header_omits_when_only_whitespace_supplied() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/ping"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&mock)
            .await;

        let client = Client::builder()
            .api_key("sk-ant-x")
            .base_url(mock.uri())
            .beta("   ")
            .beta("")
            .build()
            .unwrap();

        let _: Pong = client
            .execute(
                client.request_builder(reqwest::Method::GET, "/v1/ping"),
                &["  "],
            )
            .await
            .unwrap();

        let req = &mock.received_requests().await.unwrap()[0];
        assert!(
            req.headers.get("anthropic-beta").is_none(),
            "expected no anthropic-beta header when all values are whitespace"
        );
    }
}
