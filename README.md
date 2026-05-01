# claude-api

Type-safe Rust client for the [Anthropic API](https://docs.anthropic.com/).

[![Crates.io](https://img.shields.io/crates/v/claude-api.svg)](https://crates.io/crates/claude-api)
[![Documentation](https://docs.rs/claude-api/badge.svg)](https://docs.rs/claude-api)
[![CI](https://github.com/joshrotenberg/claude-api/actions/workflows/ci.yml/badge.svg)](https://github.com/joshrotenberg/claude-api/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/claude-api.svg)](https://github.com/joshrotenberg/claude-api#license)
[![MSRV](https://img.shields.io/crates/msrv/claude-api.svg)](https://github.com/joshrotenberg/claude-api)

## Workspace

| Crate | Description |
|-------|-------------|
| [`claude-api`](crates/claude-api/) | The SDK |
| [`claude-api-derive`](crates/claude-api-derive/) | `#[derive(Tool)]` proc-macro (re-exported via the `derive` feature) |
| [`claude-api-test`](crates/claude-api-test/) | Cassette-based replay + recorder for integration tests |

## Quick start

```toml
[dependencies]
claude-api = "0.5"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use claude_api::{Client, messages::CreateMessageRequest, types::ModelId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(std::env::var("ANTHROPIC_API_KEY")?);

    let resp = client
        .messages()
        .create(
            CreateMessageRequest::builder()
                .model(ModelId::SONNET_4_6)
                .max_tokens(256)
                .system("Be concise.")
                .user("What is the capital of France?")
                .build()?,
        )
        .await?;

    for block in &resp.content {
        if let claude_api::messages::ContentBlock::Known(
            claude_api::messages::KnownBlock::Text { text, .. },
        ) = block
        {
            println!("{text}");
        }
    }
    Ok(())
}
```

See [`crates/claude-api/examples/`](crates/claude-api/examples/) for
streaming, streaming-with-callbacks, tool use, derived tools, vision,
document, conversation, and the agent loop.

## API surface

Every documented Anthropic endpoint is reachable through a typed
namespace handle off [`Client`]:

| Namespace | Endpoints | Feature flag |
|-----------|-----------|--------------|
| `client.messages()` | create, count_tokens, streaming | (default) |
| `client.models()` | list, get | (default) |
| `client.batches()` | create, get, list, cancel, delete, results, wait_for | (default) |
| `client.files()` | upload, get, list, delete, download | (default) |
| `client.skills()` | full CRUD across skills + skill versions | `skills` |
| `client.user_profiles()` | create, list, get, update, create-enrollment-url | `user-profiles` |
| `client.managed_agents()` | sessions, agents, environments, vaults + credentials, memory_stores + memories + versions, resources, threads, events | `managed-agents-preview` |
| `client.admin()` | organizations, invites, users, workspaces + members, api_keys, usage_report, cost_report, rate_limits | `admin` |

## Capabilities

- **Forward-compatible types**. `ContentBlock`, `StreamEvent`,
  `ContentDelta`, `Citation`, `BuiltinTool`, `BetaHeader`,
  `SessionResource` and similar wrapper enums round-trip unknown
  variants through an `Other(Value)` arm. New API variants don't
  break older SDK builds.
- **`Retry-After` honored**. Configurable
  `RetryPolicy { max_attempts, initial_backoff, max_backoff,
  jitter, respect_retry_after }`.
- **`request-id` on every error**. Surfaced as
  `Error::request_id() -> Option<&str>`.
- **Streaming** with typed `EventStream` plus optional `on_text_delta` /
  `on_tool_use_complete` / `on_thinking_delta` / `on_message_stop` /
  `on_error` callback hooks.
- **Tool dispatch** with `ToolRegistry`, parallel-by-default
  invocation, mid-stream approval gates, cumulative cost budget,
  cancellation token.
- **`#[derive(Tool)]`** proc-macro generates the `Tool` impl from a
  struct's `JsonSchema` (under the `derive` feature).
- **`Conversation`** multi-turn helper with cumulative usage and
  optional context compaction.
- **`PricingTable`** with bundled per-model rates plus
  `cost_preview()` for pre-flight USD estimates.
- **`dry_run` mode** renders the would-be HTTP request as
  `DryRun { method, url, headers, body }` plus `to_curl()` /
  `to_curl_with_key()` helpers.
- **Auth**: API key (default), AWS Bedrock sigv4 signer (`bedrock`
  feature), pluggable via the `RequestSigner` trait.
- **Sync + async**: same surface, blocking variants under the `sync`
  feature.
- **Structured `tracing`** spans + events on every request and
  retry. Zero plumbing required.
- **Cassette-based integration tests** via
  [`claude-api-test`](crates/claude-api-test/) -- record once
  against the real API, replay deterministically in CI.

## Feature flags

| Flag | Default | Adds |
|------|---------|------|
| `async` | yes | Async client (tokio + reqwest) |
| `rustls` | yes | rustls TLS backend |
| `streaming` | yes | SSE streaming + `EventStream::aggregate` + callback hooks |
| `sync` |  | Blocking client (reqwest blocking) |
| `native-tls` |  | native-tls instead of rustls |
| `schemars-tools` |  | `Tool::from_schemars::<T: JsonSchema>` constructor |
| `derive` |  | `#[derive(Tool)]` (pulls in `claude-api-derive`) |
| `pricing` |  | `PricingTable` + cost calculation + `cost_preview` |
| `conversation` |  | Multi-turn `Conversation` helper |
| `bedrock` |  | AWS sigv4 `BedrockSigner` |
| `vertex` |  | (placeholder, not yet wired -- see [#6](https://github.com/joshrotenberg/claude-api/issues/6)) |
| `admin` |  | Admin API |
| `skills` |  | Skills API |
| `user-profiles` |  | User Profiles API |
| `managed-agents-preview` |  | Managed Agents preview |

Disable defaults if you only want the type definitions:

```toml
claude-api = { version = "0.5", default-features = false }
```

## Anti-goals

If a feature request comes in for any of these, the answer is "different
crate":

- Local prompt templating
- Multi-provider routing (OpenAI, Gemini, etc.)
- Response caching layer
- Background queues or scheduling
- Vector store integrations
- Embedding API (Anthropic does not have one; do not stub)
- Legacy `/v1/complete` Completions API

## Resources

- [Examples](crates/claude-api/examples/)
- [Changelog](crates/claude-api/CHANGELOG.md)
- [Open issues](https://github.com/joshrotenberg/claude-api/issues) --
  roadmap items, planned work, known gaps

## MSRV

Rust 1.90 or later. Edition 2024.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option. Contributions are dual-licensed under the same terms.
