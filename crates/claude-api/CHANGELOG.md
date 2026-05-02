# Changelog

All notable changes to `claude-api`, `claude-api-derive`, and
`claude-api-test` are documented here. Format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.5.3](https://github.com/joshrotenberg/claude-api/compare/v0.5.2...v0.5.3) - 2026-05-02

### Added

- *(live-tests)* add test stubs for user-profiles, skills write, batches completion, conversation, runner ([#36](https://github.com/joshrotenberg/claude-api/pull/36))
- *(vertex)* VertexSigner RequestSigner impl for GCP Vertex AI auth ([#37](https://github.com/joshrotenberg/claude-api/pull/37))
- *(messages)* type-promote stop_details and context_management ([#35](https://github.com/joshrotenberg/claude-api/pull/35))
- *(models)* type-promote ModelInfo.capabilities ([#27](https://github.com/joshrotenberg/claude-api/pull/27))
- *(managed-agents)* wire research-preview header for outcomes ([#24](https://github.com/joshrotenberg/claude-api/pull/24))

### Other

- new examples + module header upgrades (issue #15 partial) ([#28](https://github.com/joshrotenberg/claude-api/pull/28))

## [0.5.2](https://github.com/joshrotenberg/claude-api/compare/v0.5.1...v0.5.2) - 2026-05-01

### Fixed

- remove git-cliff changelog regen + restore curated CHANGELOG ([#23](https://github.com/joshrotenberg/claude-api/pull/23))

### Other

- regenerate changelog ([#22](https://github.com/joshrotenberg/claude-api/pull/22))

## [0.5.1] -- 2026-05-01

Documentation pass. No API changes.

### Documentation

- Rewrite `README.md` to be factual / badge-driven; competitor
  positioning moves to internal `CLAUDE.md` (gitignored).
- Replace one-line crate-level `lib.rs` placeholder with a full
  overview: quick start, module map, forward-compat policy,
  error-handling section, observability section.
- Beef up the three thinnest module headers (`messages/mod.rs`,
  `models/mod.rs`, `error.rs`) with endpoint tables, examples, and
  cross-links.

### Internal

- Repository layout: workspace crates moved under top-level
  `crates/` (`crates/claude-api`, `crates/claude-api-derive`,
  `crates/claude-api-test`). No effect on crates.io users.
- `[workspace.package]` and `[workspace.dependencies]` hoist
  edition / rust-version / license / repository plus 9
  cross-cutting deps to the workspace root.
- Edition bumped to 2024; MSRV bumped to Rust 1.90.
- Release automation: `release-plz` for version bumps and
  crates.io publishes.

## [0.5.0] -- 2026-05-01

The "full API surface" release. v0.5 closes the gap between
claude-api and the published Anthropic OpenAPI spec: every documented
endpoint is now reachable through a typed Rust namespace, every public
struct has been audited field-by-field against `BetaXxx` schemas, and
the live-test harness has recorded cassettes against the real API for
the cheap and read-only surfaces.

### Added

- **Admin API** (feature `admin`): full coverage of 27 endpoints across
  9 resources -- `organizations.me`, `invites.{create,list,retrieve,
  delete}`, `users.{list,retrieve,update,delete}`, `workspaces.
  {create,list,retrieve,update,archive}`, `workspace_members.{create,
  list,retrieve,update,delete}`, `api_keys.{list,retrieve,update}`,
  `usage_report.{messages,claude_code}`, `cost_report`,
  `rate_limits.{list_organization,list_workspace}`. Requires an
  admin-tier API key.
- **Skills API** (feature `skills`, beta `skills-2025-10-02`): full
  CRUD across 8 endpoints (skills + skill versions). Multipart upload
  via the same plumbing as `Files::upload`.
- **User Profiles API** (feature `user-profiles`, beta
  `user-profiles-2026-03-24`): create/list/get/update + the
  enrollment-URL flow. Open-string `TrustGrantStatus` enum. Update
  uses merge-patch metadata semantics.
- **Managed Agents preview** (feature `managed-agents-preview`, beta
  `managed-agents-2026-04-01`): sessions, agents, environments,
  vaults + credentials, memory_stores + memories + memory_versions,
  session resources (file/repo/memory_store), events (list/send/SSE
  stream), and the multi-agent threads endpoints (research-preview).
- **Typed `BetaHeader` enum** with the 23 canonical beta header values
  (`Skills`, `UserProfiles`, `FilesApi`, etc.) plus an `Other(String)`
  forward-compat fallthrough. Works directly with
  `ClientBuilder::beta(...)` via the existing `Into<String>` bound.
- **Bedrock auth** (feature `bedrock`): `BedrockSigner` (sigv4) as a
  drop-in `RequestSigner` for AWS Bedrock invocations.
- **Spec-diff tooling** at `tools/audit/spec_diff.sh` -- mechanical
  field-by-field comparison of every public Rust struct against its
  matching `BetaXxx` OpenAPI schema. Resolves `$ref` and `allOf`,
  understands `#[serde(rename = ...)]`, and filters tagged-variant
  false positives.
- **Live-test harness** in `claude-api-test`: 15 record-or-replay tests
  with 30 cassette exchanges committed. Auth headers are redacted
  before write; cassettes are safe to commit. The recorder now
  truncates by default (was: appended), so each record run produces a
  fresh cassette.

### Changed

- **`Session` struct** gains 6 spec-aligned fields:
  `agent: SessionAgent` (resolved snapshot at create time),
  `environment_id`, `vault_ids`, `metadata`, `stats`, plus the wire
  `type` discriminator.
- **`SessionUsage`**: replaces flat `cache_creation_input_tokens` with
  a structured `cache_creation: Option<CacheCreationUsage>` carrying
  `ephemeral_5m_input_tokens` + `ephemeral_1h_input_tokens` per the
  spec.
- **`crate::types::Usage`** gains `inference_geo: Option<String>`
  (open string) -- the live API emits this on every messages.create
  response; spec-diff didn't catch it.
- **`Agent.description` and `Agent.system`** relax from `String` to
  `Option<String>`. Same change on `SessionAgent`. Wire reality: the
  API returns JSON `null` for these fields when no value was set at
  create time. **Breaking** for callers that read those fields.
- **`Memory` and `MemoryVersion`** restructured per spec:
  `size_bytes` -> `content_size_bytes`, gain `memory_store_id` /
  `content_sha256` / per-memory `memory_version_id`. `MemoryVersion`
  replaces `redacted: bool` with `redacted_at` + `redacted_by` (typed
  `MemoryActor` tagged-union: `SessionActor` | `ApiActor` |
  `UserActor`). Adds typed `MemoryVersionOperation` enum.
- **`Credential`** restructured: `auth_type` + `mcp_server_url` flat
  fields collapse into a typed `auth: CredentialAuthResponse` union
  (McpOauth + StaticBearer + Other) per the spec.
- **`Environment`** adds `description` and `metadata` fields; gains
  the missing `update` endpoint (POST /v1/environments/{id}).
- **`MemoryStore`** adds `metadata`.
- **`Sessions::update`** added (POST /v1/sessions/{id}, was missing).
- **`Resources::retrieve`** added (GET
  /v1/sessions/{id}/resources/{rid}, was missing).
- **`SessionEvent`** gains a `SessionDeleted` variant for the
  documented `session.deleted` event type.
- **`AgentToolUseEvent`** gains `evaluated_permission:
  Option<AgentEvaluatedPermission>` (Allow | Ask | Deny).
- **`SpanModelRequestEndEvent`** gains `is_error: bool`.
- **`Message`** gains `stop_details` and `context_management` fields
  (preserved as `serde_json::Value` pending typed promotion).
- **`FileMetadata`** gains `scope: Option<FileScope>` (session
  scoping for files written by an agent under
  `/mnt/session/outputs/`).
- **`ModelInfo`** gains `max_tokens`, `max_input_tokens`,
  `capabilities` (last is `Value` pending typed promotion).
- **`GitHubRepositoryResource`** gains a typed `RepositoryCheckout`
  (Branch by name | Commit by SHA) plus `created_at` / `updated_at`.
- **`MemoryStoreResource`** gains `name` / `description` / `mount_path`.
- **`FileResource`** gains `created_at` / `updated_at`.

### Removed

- The speculative `SessionOutcomeEvaluated` event variant -- not in
  the spec or the published guide. The documented terminal-state
  signal is `span.outcome_evaluation_end` (kept).

### Fixed

- The cassette recorder appended to existing files; now truncates on
  start. (Caused stale 401 entries to accumulate during early
  recording sessions.)

### Notes

- Outcomes (`user.define_outcome`, `span.outcome_evaluation_*`) and
  multi-agent threads remain reachable through their typed events but
  are not surfaced in the OpenAPI spec; both are research-preview
  features documented in `/docs/en/managed-agents/{define-outcomes,
  multi-agent}` guide pages.
- `user_profiles.list` returns 404 against orgs that aren't enrolled
  in user_profiles. The endpoint shape is correct; this is an
  enrollment gate, not an SDK bug.

### Internal

- Spec-diff: 23/28 audited public structs match the OpenAPI spec
  field-by-field. The 5 remaining "extras" are intentional (3
  research-preview surfaces, 1 write-only secret, 1 spec
  documentation gap).
- 506 lib unit tests + 15 live-replay tests workspace-wide. Clippy +
  fmt clean across all feature combinations.

[0.5.0]: https://github.com/joshrotenberg/claude-api/releases/tag/v0.5.0
[0.5.1]: https://github.com/joshrotenberg/claude-api/releases/tag/v0.5.1
