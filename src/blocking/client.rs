//! Synchronous HTTP client.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::auth::ApiKey;
use crate::error::{Error, Result};
use crate::retry::RetryPolicy;

/// Synchronous HTTP client for the Anthropic API.
///
/// Counterpart to [`crate::Client`]; same builder shape, same retry policy,
/// same error mapping. Cheap to [`Clone`] (an `Arc<Inner>` under the hood).
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    api_key: ApiKey,
    base_url: String,
    http: reqwest::blocking::Client,
    user_agent: String,
    betas: Vec<String>,
    retry: RetryPolicy,
}

impl Client {
    /// Construct a client with default settings and the given API key.
    ///
    /// # Panics
    ///
    /// Panics if reqwest fails to build its default blocking HTTP client
    /// (extremely unusual; would indicate a broken TLS stack). Use
    /// [`Client::builder`] for a fallible alternative.
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
    pub fn messages(&self) -> super::Messages<'_> {
        super::Messages::new(self)
    }

    /// Namespace handle for the Models API.
    pub fn models(&self) -> super::Models<'_> {
        super::Models::new(self)
    }

    /// Build a [`reqwest::blocking::RequestBuilder`] preloaded with the
    /// per-request authentication, version, and user-agent headers.
    pub(crate) fn request_builder(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::blocking::RequestBuilder {
        let url = format!("{}{}", self.inner.base_url, path);
        self.inner
            .http
            .request(method, url)
            .header("x-api-key", self.inner.api_key.as_str())
            .header("anthropic-version", crate::ANTHROPIC_VERSION)
            .header(reqwest::header::USER_AGENT, &self.inner.user_agent)
    }

    /// Send a prepared request synchronously. Mirrors the async
    /// [`crate::Client::execute`] -- same header merging, same error
    /// mapping. No retries; use [`Self::execute_with_retry`] for that.
    pub(crate) fn execute<R: DeserializeOwned>(
        &self,
        mut builder: reqwest::blocking::RequestBuilder,
        per_request_betas: &[&str],
    ) -> Result<R> {
        if let Some(joined) = merge_betas(&self.inner.betas, per_request_betas) {
            builder = builder.header("anthropic-beta", joined);
        }

        let response = builder.send()?;
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

        let bytes = response.bytes()?;

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

    /// Send a request with retries. `make_request` is called once per
    /// attempt to produce a fresh [`reqwest::blocking::RequestBuilder`].
    pub(crate) fn execute_with_retry<R, F>(
        &self,
        mut make_request: F,
        per_request_betas: &[&str],
    ) -> Result<R>
    where
        R: DeserializeOwned,
        F: FnMut() -> reqwest::blocking::RequestBuilder,
    {
        let policy = &self.inner.retry;
        let mut attempt: u32 = 1;
        loop {
            let builder = make_request();
            match self.execute(builder, per_request_betas) {
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
                    std::thread::sleep(backoff);
                    attempt += 1;
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn betas(&self) -> &[String] {
        &self.inner.betas
    }
}

/// Merge client-level and per-request beta values into a single comma-joined
/// header value. Mirrors the async-side helper; trims and skips empties,
/// preserves order, no dedup.
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
#[derive(Debug, Default)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    user_agent: Option<String>,
    timeout: Option<Duration>,
    betas: Vec<String>,
    retry: Option<RetryPolicy>,
    http: Option<reqwest::blocking::Client>,
}

impl ClientBuilder {
    /// API key; required.
    #[must_use]
    pub fn api_key(mut self, k: impl Into<String>) -> Self {
        self.api_key = Some(k.into());
        self
    }

    /// Override the base URL. Useful for proxies and `wiremock`-based tests.
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Append an `anthropic-beta` value. Repeatable; values are comma-joined.
    #[must_use]
    pub fn beta(mut self, header_value: impl Into<String>) -> Self {
        self.betas.push(header_value.into());
        self
    }

    /// Per-request timeout applied to the underlying reqwest blocking client.
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

    /// Supply your own [`reqwest::blocking::Client`].
    #[must_use]
    pub fn http_client(mut self, c: reqwest::blocking::Client) -> Self {
        self.http = Some(c);
        self
    }

    /// Override the `User-Agent` header. Defaults to `claude-api-rs/<version>`.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Construct the [`Client`]. Returns [`Error::InvalidConfig`] if the API
    /// key is missing.
    pub fn build(self) -> Result<Client> {
        let api_key = self
            .api_key
            .ok_or_else(|| Error::InvalidConfig("api_key is required".into()))?;

        let http = if let Some(c) = self.http {
            c
        } else {
            let mut builder = reqwest::blocking::Client::builder();
            if let Some(t) = self.timeout {
                builder = builder.timeout(t);
            }
            builder.build()?
        };

        let inner = Inner {
            api_key: ApiKey::new(api_key),
            base_url: self
                .base_url
                .unwrap_or_else(|| crate::DEFAULT_BASE_URL.to_owned()),
            http,
            user_agent: self
                .user_agent
                .unwrap_or_else(|| crate::USER_AGENT.to_owned()),
            betas: self.betas,
            retry: self.retry.unwrap_or_default(),
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

    #[test]
    fn build_requires_api_key() {
        let err = Client::builder().build().unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
    }

    #[test]
    fn client_is_cheap_to_clone() {
        let c1 = Client::new("sk-ant-x");
        let c2 = c1.clone();
        assert!(Arc::ptr_eq(&c1.inner, &c2.inner));
    }

    #[test]
    fn builder_collects_betas_in_order() {
        let client = Client::builder()
            .api_key("sk-ant-x")
            .beta("a")
            .beta("b")
            .build()
            .unwrap();
        assert_eq!(client.betas(), &["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn merge_betas_filters_empties_and_trims() {
        assert_eq!(
            merge_betas(&["  a  ".into(), String::new()], &["", "b\n"]).as_deref(),
            Some("a,b")
        );
        assert_eq!(merge_betas(&[], &[]), None);
    }
}
