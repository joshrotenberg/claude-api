//! `DryRun` -- preview the HTTP request that would be sent, without firing it.
//!
//! Renders the same body, URL, and headers (auth + version + user-agent +
//! `anthropic-beta`) the live client would produce. Useful for:
//!
//! - inspecting the rendered JSON body during development,
//! - reproducing a request as a curl command in support tickets,
//! - asserting on the wire-shape of a request in tests.
//!
//! Obtain via [`Messages::dry_run`](crate::messages::Messages::dry_run) and
//! related methods.

#![cfg(feature = "async")]

use http::HeaderMap;

/// A rendered HTTP request that has not been sent.
///
/// Contains the exact method, URL, headers, and body the client would
/// transmit. The [`Debug`](std::fmt::Debug) impl redacts auth headers; the
/// raw values are still accessible via [`Self::headers`] for callers that
/// need them.
#[non_exhaustive]
#[derive(Clone)]
pub struct DryRun {
    /// HTTP method.
    pub method: reqwest::Method,
    /// Fully-qualified URL the request would be sent to.
    pub url: String,
    /// All headers, including `x-api-key` and `anthropic-version`.
    pub headers: HeaderMap,
    /// Decoded JSON request body.
    pub body: serde_json::Value,
}

impl DryRun {
    /// The rendered JSON request body.
    #[must_use]
    pub fn body(&self) -> &serde_json::Value {
        &self.body
    }

    /// Pretty-printed JSON body. Convenience over `serde_json::to_string_pretty`.
    #[must_use]
    pub fn body_pretty(&self) -> String {
        // unwrap: serializing a serde_json::Value cannot fail
        serde_json::to_string_pretty(&self.body).unwrap_or_default()
    }

    /// Render as a `curl` command. Auth headers (`x-api-key`,
    /// `authorization`) are replaced with `<REDACTED>`. Suitable for sharing
    /// in bug reports.
    #[must_use]
    pub fn to_curl(&self) -> String {
        self.to_curl_inner(None)
    }

    /// Render as a `curl` command with the given API key inlined for
    /// `x-api-key`. Use this when you intend to actually run the resulting
    /// command. Anything in `authorization` is still redacted.
    #[must_use]
    pub fn to_curl_with_key(&self, api_key: &str) -> String {
        self.to_curl_inner(Some(api_key))
    }

    fn to_curl_inner(&self, api_key: Option<&str>) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("curl -X ");
        out.push_str(self.method.as_str());
        out.push(' ');
        push_shell_quoted(&mut out, &self.url);
        for (name, value) in &self.headers {
            let name_str = name.as_str();
            let value_str = match name_str {
                "x-api-key" => api_key.unwrap_or("<REDACTED>"),
                "authorization" => "<REDACTED>",
                _ => value.to_str().unwrap_or("<binary>"),
            };
            out.push_str(" \\\n  -H ");
            push_shell_quoted(&mut out, &format!("{name_str}: {value_str}"));
        }
        out.push_str(" \\\n  -d ");
        push_shell_quoted(&mut out, &self.body_pretty());
        out
    }
}

impl std::fmt::Debug for DryRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut redacted = self.headers.clone();
        for name in ["x-api-key", "authorization"] {
            if redacted.contains_key(name) {
                redacted.insert(name, "<REDACTED>".parse().unwrap());
            }
        }
        f.debug_struct("DryRun")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &redacted)
            .field("body", &self.body)
            .finish()
    }
}

/// Single-quote a string for safe inclusion in a shell command.
fn push_shell_quoted(out: &mut String, s: &str) {
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            // Close, escape literal apostrophe, reopen.
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn sample() -> DryRun {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "sk-ant-secret".parse().unwrap());
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        DryRun {
            method: reqwest::Method::POST,
            url: "https://api.anthropic.com/v1/messages".into(),
            headers,
            body: json!({"model": "claude-sonnet-4-6", "max_tokens": 8}),
        }
    }

    #[test]
    fn body_pretty_returns_indented_json() {
        let dr = sample();
        let p = dr.body_pretty();
        assert!(p.contains("\"model\": \"claude-sonnet-4-6\""));
        assert!(p.contains('\n'));
    }

    #[test]
    fn to_curl_redacts_api_key_by_default() {
        let dr = sample();
        let curl = dr.to_curl();
        assert!(curl.contains("x-api-key: <REDACTED>"));
        assert!(!curl.contains("sk-ant-secret"));
        assert!(curl.contains("anthropic-version: 2023-06-01"));
        assert!(curl.starts_with("curl -X POST 'https://api.anthropic.com/v1/messages'"));
    }

    #[test]
    fn to_curl_with_key_inlines_key() {
        let dr = sample();
        let curl = dr.to_curl_with_key("sk-ant-real");
        assert!(curl.contains("x-api-key: sk-ant-real"));
    }

    #[test]
    fn debug_redacts_auth_headers() {
        let dr = sample();
        let s = format!("{dr:?}");
        assert!(!s.contains("sk-ant-secret"));
        assert!(s.contains("<REDACTED>"));
    }

    #[test]
    fn debug_passes_through_non_auth_headers() {
        let dr = sample();
        let s = format!("{dr:?}");
        assert!(s.contains("anthropic-version"));
    }

    #[test]
    fn shell_quoting_escapes_single_quotes() {
        let mut out = String::new();
        push_shell_quoted(&mut out, "it's");
        assert_eq!(out, "'it'\\''s'");
    }
}
