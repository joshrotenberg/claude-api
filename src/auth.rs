//! Authentication primitives.
//!
//! v0.1 supports API-key auth only via [`ApiKey`]. AWS Bedrock and GCP Vertex
//! variants are stubbed behind the `bedrock` and `vertex` feature flags and
//! are not implemented yet.

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
