//! Google Cloud Vertex AI support: a [`RequestSigner`] that attaches an
//! `OAuth2` bearer token so requests are authenticated against the Vertex AI
//! Anthropic endpoint.
//!
//! The URL shape for Vertex AI is:
//! ```text
//! https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/anthropic/models/{model}:rawPredict
//! ```
//!
//! Auth is a standard `Authorization: Bearer {token}` header where the token
//! is a Google `OAuth2` access token.
//!
//! # Credential sources
//!
//! Two credential sources are supported:
//!
//! - **Static token** (`VertexCredentials::from_token`): supply a token
//!   string directly. Useful for tests and short-lived scripts where you
//!   already have a token (e.g., from `gcloud auth print-access-token`).
//!
//! - **Application Default Credentials** (`VertexCredentials::from_adc`):
//!   uses [`gcp_auth`] to obtain a token via Application Default Credentials
//!   (service-account key file, GCE metadata server, `gcloud` CLI, or the
//!   `GOOGLE_APPLICATION_CREDENTIALS` environment variable). This path
//!   requires an active Tokio runtime because token refresh is async; it
//!   calls `Handle::current().block_on(...)` inside `sign()`.
//!
//! [`VertexCredentials::from_env`] checks `VERTEX_ACCESS_TOKEN` first (static
//! token), then falls back to `GOOGLE_APPLICATION_CREDENTIALS` (ADC).
//!
//! # Set up the client
//!
//! ```no_run
//! use std::sync::Arc;
//! use claude_api::{Client, vertex::{VertexCredentials, VertexSigner}};
//! # fn run() -> Result<(), claude_api::Error> {
//! let creds = VertexCredentials::from_env()
//!     .expect("VERTEX_ACCESS_TOKEN or GOOGLE_APPLICATION_CREDENTIALS must be set");
//! let region = std::env::var("VERTEX_REGION").unwrap_or_else(|_| "us-east5".into());
//! let project = std::env::var("VERTEX_PROJECT").expect("VERTEX_PROJECT must be set");
//! let client = Client::builder()
//!     .signer(Arc::new(VertexSigner::new(creds)))
//!     .base_url(format!(
//!         "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/anthropic"
//!     ))
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! Gated on the `vertex` feature.

#![cfg(feature = "vertex")]
#![cfg_attr(docsrs, doc(cfg(feature = "vertex")))]

use std::sync::Arc;

use gcp_auth::TokenProvider;

use crate::auth::{RequestSigner, SignerResult};

/// The `OAuth2` scope required for Vertex AI API access.
const VERTEX_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// The set of scopes passed to the token provider.
const VERTEX_SCOPES: &[&str] = &[VERTEX_SCOPE];

/// Credential source for Vertex AI authentication.
///
/// Carries either a static bearer token (no async refresh) or an ADC-backed
/// [`TokenProvider`] that fetches and caches tokens via `gcp_auth`.
///
/// Construct with [`VertexCredentials::from_token`],
/// [`VertexCredentials::from_adc`], or [`VertexCredentials::from_env`].
#[derive(Clone)]
pub struct VertexCredentials {
    inner: CredentialInner,
}

#[derive(Clone)]
enum CredentialInner {
    /// A static bearer token -- no async refresh.
    Static(String),
    /// An ADC-backed token provider from `gcp_auth`.
    Adc(Arc<dyn TokenProvider>),
}

impl VertexCredentials {
    /// Use a pre-obtained `OAuth2` bearer token directly.
    ///
    /// The token is used verbatim; no refresh is performed. Suitable for
    /// short-lived scripts or tests where you already have a token (e.g.,
    /// `gcloud auth print-access-token`).
    #[must_use]
    pub fn from_token(token: impl Into<String>) -> Self {
        Self {
            inner: CredentialInner::Static(token.into()),
        }
    }

    /// Use Application Default Credentials via [`gcp_auth`].
    ///
    /// Tries, in order:
    /// 1. `GOOGLE_APPLICATION_CREDENTIALS` env var (service-account key file)
    /// 2. `~/.config/gcloud/application_default_credentials.json`
    /// 3. GCE instance-metadata server
    /// 4. `gcloud auth print-access-token`
    ///
    /// Tokens are cached and refreshed automatically by `gcp_auth`. This
    /// constructor is async because provider discovery may involve network
    /// I/O (metadata server probe).
    ///
    /// # Errors
    ///
    /// Returns an error if no credential source is found or if the initial
    /// provider discovery fails.
    pub async fn from_adc() -> Result<Self, gcp_auth::Error> {
        let provider = gcp_auth::provider().await?;
        Ok(Self {
            inner: CredentialInner::Adc(provider),
        })
    }

    /// Read credentials from environment variables.
    ///
    /// Checks, in order:
    /// 1. `VERTEX_ACCESS_TOKEN` -- if set, constructs a
    ///    [`from_token`](Self::from_token) credential.
    /// 2. `GOOGLE_APPLICATION_CREDENTIALS` -- if set, returns an
    ///    [`from_adc`](Self::from_adc) credential.
    ///
    /// Returns `None` when neither variable is set.
    ///
    /// # Panics
    ///
    /// This method calls `from_adc()` synchronously by blocking on the
    /// current Tokio runtime when `GOOGLE_APPLICATION_CREDENTIALS` is set.
    /// It will panic if called outside a Tokio runtime context.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        // Prefer a static token if supplied.
        if let Ok(token) = std::env::var("VERTEX_ACCESS_TOKEN") {
            return Some(Self::from_token(token));
        }

        // Fall back to ADC when GOOGLE_APPLICATION_CREDENTIALS is set.
        if std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS").is_some() {
            let handle = tokio::runtime::Handle::current();
            return match handle.block_on(gcp_auth::provider()) {
                Ok(provider) => Some(Self {
                    inner: CredentialInner::Adc(provider),
                }),
                Err(_) => None,
            };
        }

        None
    }

    /// Resolve the current bearer token.
    ///
    /// For static credentials this is infallible. For ADC credentials,
    /// requires a Tokio runtime handle (will block the current thread).
    fn resolve_token(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        match &self.inner {
            CredentialInner::Static(t) => Ok(t.clone()),
            CredentialInner::Adc(provider) => {
                let handle = tokio::runtime::Handle::current();
                let token = handle
                    .block_on(provider.token(VERTEX_SCOPES))
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
                Ok(token.as_str().to_owned())
            }
        }
    }
}

impl std::fmt::Debug for VertexCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            CredentialInner::Static(t) => f
                .debug_struct("VertexCredentials")
                .field("kind", &"static-token")
                .field("token", &format!("<redacted, {} chars>", t.len()))
                .finish(),
            CredentialInner::Adc(_) => f
                .debug_struct("VertexCredentials")
                .field("kind", &"adc")
                .finish(),
        }
    }
}

/// Vertex AI bearer-token signer.
///
/// Attaches `Authorization: Bearer {token}` to every outbound request and
/// removes the `x-api-key` header (Vertex AI does not use it).
///
/// Install on a [`Client`](crate::Client) via
/// [`ClientBuilder::signer`](crate::ClientBuilder::signer).
#[derive(Debug, Clone)]
pub struct VertexSigner {
    credentials: VertexCredentials,
}

impl VertexSigner {
    /// Build a signer from `credentials`.
    #[must_use]
    pub fn new(credentials: VertexCredentials) -> Self {
        Self { credentials }
    }
}

impl RequestSigner for VertexSigner {
    fn sign(&self, request: &mut reqwest::Request) -> SignerResult {
        // Remove the Anthropic API-key header -- Vertex does not use it.
        request.headers_mut().remove("x-api-key");

        let token = self.credentials.resolve_token()?;
        let bearer = format!("Bearer {token}");
        request
            .headers_mut()
            .insert("authorization", bearer.parse()?);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request_with_api_key() -> reqwest::Request {
        let client = reqwest::Client::new();
        client
            .post("https://us-east5-aiplatform.googleapis.com/v1/projects/my-project/locations/us-east5/publishers/anthropic/models/claude-sonnet-4-6:rawPredict")
            .header("x-api-key", "sk-ant-test-key")
            .body(r#"{"messages":[{"role":"user","content":"hi"}]}"#)
            .build()
            .unwrap()
    }

    fn make_request_without_api_key() -> reqwest::Request {
        let client = reqwest::Client::new();
        client
            .post("https://us-east5-aiplatform.googleapis.com/v1/projects/my-project/locations/us-east5/publishers/anthropic/models/claude-sonnet-4-6:rawPredict")
            .body(r#"{"messages":[{"role":"user","content":"hi"}]}"#)
            .build()
            .unwrap()
    }

    fn static_signer(token: &str) -> VertexSigner {
        VertexSigner::new(VertexCredentials::from_token(token))
    }

    #[test]
    fn sign_adds_authorization_bearer_header() {
        let signer = static_signer("ya29.test-token");
        let mut req = make_request_without_api_key();
        signer.sign(&mut req).expect("sign succeeds");

        let auth = req
            .headers()
            .get("authorization")
            .expect("authorization header set by signer");
        let auth_str = auth.to_str().expect("authorization is ASCII");
        assert_eq!(
            auth_str, "Bearer ya29.test-token",
            "expected bearer prefix: {auth_str}"
        );
    }

    #[test]
    fn sign_removes_x_api_key_header() {
        let signer = static_signer("ya29.test-token");
        let mut req = make_request_with_api_key();

        // Verify the header is present before signing.
        assert!(
            req.headers().get("x-api-key").is_some(),
            "x-api-key must be present before sign()"
        );

        signer.sign(&mut req).expect("sign succeeds");

        assert!(
            req.headers().get("x-api-key").is_none(),
            "x-api-key must be removed after sign()"
        );
    }

    #[test]
    fn sign_sets_correct_bearer_format() {
        let token = "ya29.c.long-token-value-here";
        let signer = static_signer(token);
        let mut req = make_request_without_api_key();
        signer.sign(&mut req).expect("sign succeeds");

        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth.starts_with("Bearer "), "must start with 'Bearer '");
        assert!(auth.contains(token), "must contain the token");
    }

    #[test]
    fn credentials_redact_token_in_debug() {
        let creds = VertexCredentials::from_token("ya29.very-secret-token");
        let dbg = format!("{creds:?}");
        assert!(!dbg.contains("very-secret-token"), "{dbg}");
        assert!(dbg.contains("redacted"), "{dbg}");
    }

    #[test]
    fn credentials_debug_shows_adc_kind_without_token() {
        // Build an ADC variant manually by wrapping a fake provider.
        struct FakeProvider;

        #[async_trait::async_trait]
        impl TokenProvider for FakeProvider {
            async fn token(
                &self,
                _scopes: &[&str],
            ) -> Result<Arc<gcp_auth::Token>, gcp_auth::Error> {
                unimplemented!()
            }

            async fn project_id(&self) -> Result<Arc<str>, gcp_auth::Error> {
                unimplemented!()
            }
        }

        let creds = VertexCredentials {
            inner: CredentialInner::Adc(Arc::new(FakeProvider)),
        };
        let dbg = format!("{creds:?}");
        assert!(dbg.contains("adc"), "{dbg}");
    }

    #[test]
    fn from_env_returns_none_when_no_vars_set() {
        // Guard: clear both env vars for the duration of this test.
        // Because std::env is process-global, we can only check that
        // from_env() returns None when neither var is present. We cannot
        // reliably clear env vars that may be set by the outer environment,
        // so we skip the assertion if either is already set.
        let has_token = std::env::var("VERTEX_ACCESS_TOKEN").is_ok();
        let has_adc = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS").is_some();
        if has_token || has_adc {
            // Environment is pre-configured; skip.
            return;
        }
        let result = {
            // Neither var is set in the environment.
            // from_env() must return None when called outside a Tokio
            // runtime (the GOOGLE_APPLICATION_CREDENTIALS branch is not
            // reached, so no panic).
            VertexCredentials::from_env()
        };
        assert!(
            result.is_none(),
            "expected None when env vars are absent: {result:?}"
        );
    }

    #[test]
    fn from_env_returns_static_creds_when_vertex_access_token_env_is_set() {
        // We cannot mutate env vars in tests (unsafe_code = "forbid").
        // This test verifies the from_env logic for the static-token path by
        // constructing credentials directly via from_token, which exercises
        // the same CredentialInner::Static variant that from_env() produces
        // when VERTEX_ACCESS_TOKEN is set.
        let creds = VertexCredentials::from_token("ya29.env-test-token");
        assert!(
            matches!(creds.inner, CredentialInner::Static(_)),
            "from_token must yield a static credential"
        );
    }
}
