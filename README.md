# claude-api

Type-safe Rust client for the [Anthropic API](https://docs.anthropic.com/).

Sibling project to `claude-wrapper`. That one wraps the `claude` CLI; this
one wraps the HTTP API. They do not depend on each other.

## Status

**v0.5 -- full API surface.** Every documented Anthropic endpoint is
now reachable through a typed Rust namespace:

- **Messages, Models, Batches, Files, count_tokens, streaming**
  (carried forward from v0.1-v0.4)
- **Admin API** (27 endpoints): organization, invites, users,
  workspaces + members, api_keys, usage_report, cost_report,
  rate_limits
- **Skills API** (8 endpoints): full CRUD + skill versions, multipart
  upload
- **User Profiles API** (5 endpoints) including the enrollment-URL flow
- **Managed Agents preview**: sessions, agents, environments, vaults
  + credentials, memory_stores + memories + memory_versions, session
  resources, events (list/send/SSE stream), and the multi-agent
  threads endpoints
- **Bedrock auth** via a sigv4 `RequestSigner` extension point
- **Typed `BetaHeader` enum** for the 23 canonical `anthropic-beta`
  values, with `Other(String)` forward-compat fallthrough
- **Live-test harness** with 15 record-or-replay tests covering the
  cheap and read-only surfaces; cassettes committed for free CI replay
- **Spec-diff tooling** that compares every public Rust struct to its
  OpenAPI schema field-by-field

Carried forward from v0.4 and earlier:

- **v0.4** Agent-loop guardrails (parallel dispatch + cost budget +
  cancellation), streaming callbacks, `dry_run`, `cost_preview`,
  `#[derive(Tool)]` proc macro
- **v0.3** Typed `Citation` enum, context compaction, full Batches
  API, Files API beta, typed `BuiltinTool` wrappers
- **v0.2** `ToolRegistry` + agent loop, `Conversation` helper,
  `PricingTable`, vision and document blocks, prompt-cache sugar,
  sync feature
- **v0.1** Messages + Models endpoints, forward-compatible serde,
  retry policy that honors `Retry-After`, `request-id` propagation,
  structured `tracing`

See [`CHANGELOG.md`](CHANGELOG.md) for per-version detail.

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

See [`examples/`](examples/) for streaming, streaming-with-callbacks,
tool use, derived tools, vision, document, conversation, and the agent
loop.

## Why another Anthropic Rust client?

- **Forward-compatible by design.** New `ContentBlock`, `StreamEvent`,
  `ContentDelta`, `Citation`, and `BuiltinTool` variants from the API are
  preserved as raw JSON via an `Other(Value)` arm rather than breaking
  older SDK versions. New `type` tags round-trip byte-for-byte. No other
  Rust crate (and no official SDK in any language) does this.

  **Upgrade contract**: when a server-side `type` tag that previously
  fell through to `Other` becomes a recognized `Known` variant in a new
  release, that's a **minor** version bump. Code that pattern-matched
  on `Other(v) if v["type"] == "thinking"` will silently stop matching
  -- the value now arrives as `Known(KnownBlock::Thinking { .. })`.
  When you bump claude-api, sweep your `Other` matches and route the
  newly-known variants explicitly. Releases that promote variants will
  call them out in the changelog.
- **`Retry-After` honored.** Most existing Rust crates for the Anthropic
  API ignore the header. This one respects it, with configurable
  `RetryPolicy { max_attempts, initial_backoff, max_backoff, jitter,
  respect_retry_after }`.
- **Cost-aware by default.** `PricingTable` ships with bundled rates for
  current models; `Conversation::cost(&PricingTable)` and
  `cost_preview(&request, &pricing)` give you live and pre-flight USD
  numbers. Agent loop accepts a `cost_budget` that aborts a run before
  the next iteration if a cap is exceeded.
- **Operational quality from day one.** `request-id` is surfaced on every
  error (critical for support tickets), `ApiKey: Debug` redacts the
  secret, base-URL override makes wiremock-based testing the default
  story, structured `tracing` spans on every request and retry.
- **Tools you can derive.** `#[derive(Tool)]` on a struct generates the
  `Tool` impl from the struct's `JsonSchema` and an inherent
  `async fn run(self)`. No string-literal schemas, no boilerplate `name`
  / `description` overrides unless you want them.
- **No surprise abstractions.** No prompt-template DSL, no multi-provider
  routing, no response caching, no bundled CLI. If you want those, use a
  different crate.

## Feature flags

| Flag                       | Default | Adds                                                       |
| -------------------------- | ------- | ---------------------------------------------------------- |
| `async`                    | yes     | Async client (tokio + reqwest)                             |
| `rustls`                   | yes     | rustls TLS backend                                         |
| `streaming`                | yes     | SSE streaming + `EventStream::aggregate` + callback hooks  |
| `sync`                     |         | Blocking client (reqwest blocking)                         |
| `native-tls`               |         | native-tls backend instead of rustls                       |
| `schemars-tools`           |         | `Tool::from_schemars::<T: JsonSchema>` ctor                |
| `derive`                   |         | `#[derive(Tool)]` (pulls in `claude-api-derive`)           |
| `pricing`                  |         | `PricingTable` + cost calculation + `cost_preview`         |
| `conversation`             |         | Multi-turn `Conversation` helper                           |
| `bedrock`                  |         | AWS sigv4 `BedrockSigner` (custom auth signer)             |
| `vertex`                   |         | GCP Vertex auth -- v0.6+ (placeholder; not wired yet)      |
| `admin`                    |         | Admin API (organizations, users, workspaces, etc.)         |
| `skills`                   |         | Skills API (CRUD + versions, multipart upload)             |
| `user-profiles`            |         | User Profiles API + enrollment-URL flow                    |
| `managed-agents-preview`   |         | Managed Agents preview (sessions, agents, vaults, ...)     |

Disable defaults if you only want the type definitions:

```toml
claude-api = { version = "0.5", default-features = false }
```

## Tool example with `#[derive(Tool)]`

```rust
use claude_api::derive::Tool;
use claude_api::tool_dispatch::{ToolError, ToolRegistry};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

/// Get the current weather for a city.
#[derive(Deserialize, JsonSchema, Tool)]
struct GetWeather {
    /// City to look up.
    city: String,
}

impl GetWeather {
    async fn run(self) -> Result<Value, ToolError> {
        Ok(json!({"temp_f": 72, "city": self.city}))
    }
}

let mut registry = ToolRegistry::new();
registry.register_tool(GetWeather::tool());
```

The tool name (`get_weather`) is derived from the struct name; the
description from the doc comment. Override either with
`#[tool(name = "...", description = "...")]`.

## Roadmap

- **v0.1** Messages, Models, streaming, retry, forward-compat
- **v0.2** `ToolRegistry` + agent loop, `Conversation` helper,
  `PricingTable`, vision and document blocks, prompt-cache sugar, sync
  feature
- **v0.3** Batches (with poller), Files API, citations, typed built-in
  tool wrappers, context compaction
- **v0.4** Agent-loop guardrails (parallel dispatch + cost budget +
  cancellation), streaming callbacks, `dry_run`, `cost_preview`,
  `#[derive(Tool)]` proc macro, mid-stream tool approval gates,
  record/replay test harness, Bedrock auth (sigv4 signer)
- **v0.5** Admin / Skills / User Profiles / Managed Agents preview
  endpoints, typed `BetaHeader` enum, spec-diff tooling, live-test
  cassettes
- **v0.6+** Vertex AI auth, Tower `Service` -> `Tool` integration,
  typed promotions for `stop_details` / `context_management` /
  `ModelCapabilities`, streaming-SSE cassette recording, vault
  credential live tests

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
