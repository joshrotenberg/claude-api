//! Authentication primitives and the [`RequestSigner`] hook.
//!
//! [`ApiKey`] wraps a secret with a redacting [`Debug`] impl. The
//! [`RequestSigner`] trait is the extension point: every outbound request
//! is handed to a signer just before transmission. Default behavior is
//! [`ApiKeySigner`] (adds `x-api-key`); behind the `bedrock` feature,
//! [`BedrockSigner`](crate::bedrock::BedrockSigner) signs requests with
//! AWS sigv4. A custom signer can be installed via
//! [`ClientBuilder::signer`](crate::ClientBuilder::signer).

use std::fmt;

/// Anthropic API key.
///
/// Wraps the underlying string and redacts it from [`Debug`] output to
/// reduce the chance of leaking the key into logs or panic messages.
///
/// ```
/// use claude_api::auth::ApiKey;
/// let k = ApiKey::new("sk-ant-secretvalue");
/// // Debug output never includes the secret bytes.
/// let dbg = format!("{k:?}");
/// assert!(!dbg.contains("secretvalue"));
/// ```
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wrap an API key string.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Borrow the underlying key bytes. Crate-internal so the secret stays
    /// inside this crate unless the caller explicitly opts out via
    /// [`Self::expose`].
    #[cfg(any(feature = "async", feature = "sync"))]
    #[allow(dead_code)] // used by async client and the blocking submodule
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the underlying string.
    ///
    /// Use sparingly; the wrapper exists to discourage casual leakage.
    #[must_use]
    pub fn expose(self) -> String {
        self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApiKey(<redacted, {} chars>)", self.0.len())
    }
}

impl From<String> for ApiKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ApiKey {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Result alias for [`RequestSigner::sign`]. Boxed errors so signer
/// implementations can use any error type that satisfies the bound.
pub type SignerResult<T = ()> =
    std::result::Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;

/// Hook called for every outbound HTTP request just before transmission.
///
/// Implementations install request-level authentication: the default
/// [`ApiKeySigner`] adds `x-api-key`, the optional
/// [`BedrockSigner`](crate::bedrock::BedrockSigner) adds AWS sigv4
/// signing headers. Install a custom signer via
/// [`ClientBuilder::signer`](crate::ClientBuilder::signer).
///
/// Signers run *after* per-request beta headers are merged but *before*
/// the request is sent, so the canonical body and headers are visible
/// for hashing.
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub trait RequestSigner: fmt::Debug + Send + Sync + 'static {
    /// Sign `request` in place. Return an error to abort the request
    /// before it is sent; the error is wrapped in
    /// [`Error::Signing`](crate::Error::Signing).
    fn sign(&self, request: &mut reqwest::Request) -> SignerResult;
}

/// Default signer: adds the `x-api-key` header from the wrapped key.
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
#[derive(Debug, Clone)]
pub struct ApiKeySigner {
    key: ApiKey,
}

#[cfg(feature = "async")]
impl ApiKeySigner {
    /// Wrap an [`ApiKey`].
    #[must_use]
    pub fn new(key: ApiKey) -> Self {
        Self { key }
    }
}

#[cfg(feature = "async")]
impl RequestSigner for ApiKeySigner {
    fn sign(&self, request: &mut reqwest::Request) -> SignerResult {
        request
            .headers_mut()
            .insert("x-api-key", self.key.as_str().parse()?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn debug_redacts_the_secret() {
        let k = ApiKey::new("sk-ant-very-secret-value-do-not-leak");
        let dbg = format!("{k:?}");
        assert!(!dbg.contains("secret"), "{dbg}");
        assert!(!dbg.contains("very"), "{dbg}");
        assert!(dbg.contains("redacted"), "{dbg}");
        // Length shown for sanity, not for reconstruction.
        assert!(dbg.contains(&k.0.len().to_string()), "{dbg}");
    }

    #[test]
    fn expose_returns_underlying_string() {
        let k = ApiKey::new("sk-ant-foo");
        assert_eq!(k.expose(), "sk-ant-foo");
    }

    #[test]
    fn from_string_and_str() {
        let _: ApiKey = "sk-ant-x".into();
        let _: ApiKey = String::from("sk-ant-y").into();
    }
}
