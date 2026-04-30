# claude-api

Type-safe Rust client for the [Anthropic API](https://docs.anthropic.com/).

Sibling project to `claude-wrapper`. That one wraps the `claude` CLI; this
one wraps the HTTP API. They do not depend on each other.

## Status

**v0.1 -- foundation.** Messages (`create`, `count_tokens`, `create_stream`)
and Models (`list`, `get`) endpoints are live. Forward-compatible serde,
retry policy that honors `Retry-After`, `request-id` propagation on every
error, base-URL override for testing, structured `tracing` logs.

## Quick start

```toml
[dependencies]
claude-api = "0.1"
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

See [`examples/`](examples/) for streaming, tool use, and more.

## Why another Anthropic Rust client?

- **Forward-compatible by design.** New `ContentBlock`, `StreamEvent`, and
  `ContentDelta` variants from the API are preserved as raw JSON via an
  `Other(Value)` arm rather than breaking older SDK versions. New `type`
  tags round-trip byte-for-byte.
- **`Retry-After` honored.** Most existing Rust crates for the Anthropic API
  ignore the header. This one respects it, with configurable
  `RetryPolicy { max_attempts, initial_backoff, max_backoff, jitter,
  respect_retry_after }`.
- **Operational quality from day one.** `request-id` is surfaced on every
  error (critical for support tickets), `ApiKey: Debug` redacts the secret,
  base-URL override makes wiremock-based testing the default story.
- **Comprehensive coverage roadmap.** Messages and Models in v0.1; Batches
  and Files in v0.3; Managed Agents in v0.4; Admin and Bedrock / Vertex auth
  in v0.5+.
- **No surprise abstractions.** No prompt-template DSL, no multi-provider
  routing, no response caching, no bundled CLI. If you want those, use a
  different crate.

## Feature flags

| Flag             | Default | Adds                                              |
| ---------------- | ------- | ------------------------------------------------- |
| `async`          | yes     | Async client (tokio + reqwest)                    |
| `rustls`         | yes     | rustls TLS backend                                |
| `streaming`      | yes     | SSE streaming + `Aggregator`                      |
| `sync`           |         | Blocking client (reqwest blocking)                |
| `native-tls`     |         | native-tls backend instead of rustls              |
| `schemars-tools` |         | `Tool::from_schemars::<T: JsonSchema>` ctor       |
| `pricing`        |         | Pricing table + cost calculation                  |
| `conversation`   |         | Multi-turn `Conversation` helper                  |
| `bedrock`        |         | AWS Bedrock auth -- v0.5+                         |
| `vertex`         |         | GCP Vertex auth -- v0.5+                          |
| `admin`          |         | Admin API -- v0.5+                                |
| `managed-agents` |         | Managed Agents API -- v0.4                        |

Disable defaults if you only want the type definitions:

```toml
claude-api = { version = "0.1", default-features = false }
```

## Roadmap

- **v0.1** Messages, Models, streaming, retry, forward-compat
- **v0.2** `ToolRegistry` + agent loop, `Conversation` helper, `PricingTable`,
  vision and document blocks, prompt-cache sugar, sync feature
- **v0.3** Batches (with poller), Files API, citations
- **v0.4** Managed Agents (agents, environments, sessions, vaults, memory
  stores, skills, user profiles), built-in tool wrappers
- **v0.5+** Admin API, Bedrock and Vertex auth

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

## MSRV

Rust 1.82 or later.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option. Contributions are dual-licensed under the same terms.
