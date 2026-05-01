//! Shared helpers for integration tests.
//!
//! Each `tests/*.rs` file is its own crate and pulls these in via
//! `mod common;`. Keep this module dependency-light -- everything here is
//! reused across multiple test binaries.

#![allow(dead_code)] // helpers may be unused from a given test binary

use claude_api::Client;
use wiremock::MockServer;

/// Build a [`Client`] pointed at the given wiremock server with a fixed
/// throwaway API key.
pub fn client_for(mock: &MockServer) -> Client {
    Client::builder()
        .api_key("sk-ant-integration-test")
        .base_url(mock.uri())
        .build()
        .expect("client should build with api_key + base_url")
}

/// Read a JSON fixture from `tests/fixtures/<name>` and parse it.
pub fn load_fixture_json(name: &str) -> serde_json::Value {
    let raw = load_fixture_string(name);
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("fixture {name} is not valid JSON: {e}"))
}

/// Read a fixture file as a raw UTF-8 string.
pub fn load_fixture_string(name: &str) -> String {
    let path = format!("tests/fixtures/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"))
}

/// Read an SSE corpus file from `tests/sse_corpus/<name>` as a raw string.
///
/// Corpus files preserve the literal SSE wire format: `event:` / `data:`
/// lines separated by blank lines, terminating in a blank line.
pub fn load_sse_corpus(name: &str) -> String {
    let path = format!("tests/sse_corpus/{name}");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read SSE corpus {path}: {e}"))
}
