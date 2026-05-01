# claude-api release-readiness audit

Findings from comparing `src/` against `~/claude-api-docs/` (API
reference under `/docs/en/api/...`) and `~/claude-api-docs/guides/`
(guide pages under `/docs/en/managed-agents/...`).

Status: `[ ]` open  `[x]` resolved  `[?]` decision needed

---

## Critical (correctness bugs)

### Beta-header gating

- [ ] **`MANAGED_AGENTS_RESEARCH_PREVIEW_BETA` defined but never
  referenced** (`src/managed_agents/mod.rs:96`).
  - Outcomes is research-preview per `define-outcomes.md:18` and
    every curl example uses
    `managed-agents-2026-04-01-research-preview`.
  - Multi-agent threads are *labeled* research-preview at
    `multi-agent.md:9` but every curl example uses only the base
    `managed-agents-2026-04-01`. Likely graduated; warning text
    stale.
  - **Action**: when an `Outgoing*` event tagged `user.define_outcome`
    is sent, attach research-preview. When `Sessions::retrieve`
    finds an `outcome_evaluations` field, the previous request
    should already have included research-preview. Threads CRUD: no
    extra header per the curl examples.
  - Open question: easiest implementation is to expose
    `Sessions::with_research_preview(...)` so the user opts in
    explicitly. Alternative: add to the global beta list when
    `feature = "managed-agents-preview"` is on. Need a call.

### Missing endpoints

- [ ] **`managed_agents::Agents::delete`** -- documented in
  `~/claude-api-docs/beta/agents/delete.md`, missing from
  `src/managed_agents/agents.rs`.
- [ ] **`managed_agents::Environments::update`** -- documented in
  `~/claude-api-docs/beta/environments/update.md`, missing from
  `src/managed_agents/environments.rs`.
- [ ] **`managed_agents::Sessions::update`** -- documented in
  `~/claude-api-docs/beta/sessions/update.md`, missing from
  `src/managed_agents/sessions.rs`.
- [ ] **`managed_agents::Resources::retrieve`** -- documented in
  `~/claude-api-docs/beta/sessions/resources/retrieve.md`, missing
  from `src/managed_agents/resources.rs`.

### Untyped / wrong-shape payloads

- [ ] **`SpanOutcomeEvaluationStart`** is `EventEnvelope`. Real
  shape per `define-outcomes.md:401`:
  `{ id, processed_at, outcome_id, iteration }`.

- [ ] **`SpanOutcomeEvaluationOngoing`** is `EventEnvelope`. Real
  shape per `define-outcomes.md:415`:
  `{ id, processed_at, outcome_id }`.

- [ ] **`SpanOutcomeEvaluationEnd`** is `EventEnvelope`. Real
  shape per `define-outcomes.md:436`:
  `{ id, processed_at, outcome_evaluation_start_id, outcome_id,
     result, explanation, iteration, usage }`.
  Need new closed enum `OutcomeResult { Satisfied | NeedsRevision |
  MaxIterationsReached | Failed | Interrupted }` per
  `define-outcomes.md:428` table.

- [ ] **`Session.outcome_evaluations: Vec<serde_json::Value>`**
  should be typed. Per `define-outcomes.md:467` the entries have
  at least `outcome_id` and `result`; the wider shape is plausibly
  the same as `SpanOutcomeEvaluationEnd`. Verify via live retrieve;
  if uncertain, type as `OutcomeEvaluation { outcome_id, result,
  ...rest preserved as #[serde(flatten)] HashMap }`.

- [ ] **`SessionOutcomeEvaluated`** variant
  (`session.outcome_evaluated`) is undocumented anywhere -- not in
  the API reference, not in the guide. Likely speculative -- remove
  from `SessionEvent` enum, since `span.outcome_evaluation_end` is
  the documented terminal signal.

### Missing fields

- [ ] **`Session` struct** missing 6 fields per
  `~/claude-api-docs/beta/sessions/retrieve.md:69-533`:
  - `agent: SessionAgent` (resolved snapshot at session-creation
    time -- `id`, `description`, `mcp_servers`, `model`, `name`,
    `skills`, `system`, `tools`, `version`).
  - `environment_id: String`
  - `vault_ids: Vec<String>`
  - `metadata: HashMap<String, String>`
  - `stats: SessionStats`
  - `type: String` (`"session"` discriminator)

- [ ] **`Agent` struct** missing `archived_at: Option<String>` per
  `~/claude-api-docs/beta/agents/retrieve.md:81`.

- [ ] **`SessionEvent`** missing `SessionDeleted(EventEnvelope)`
  variant for `session.deleted` per
  `~/claude-api-docs/beta/sessions/events.md` and `events/list.md`,
  `events/stream.md`.

---

## High (gaps to close before release)

- [ ] **Doc-string `agent.callable_agents` with the one-level
  delegation rule** per `multi-agent.md:224`: "Only one level of
  delegation is supported: the coordinator can call other agents,
  but those agents cannot call agents of their own."

---

## Medium

- [ ] **Add `BetaHeader::ManagedAgentsResearchPreview`** variant for
  the `managed-agents-2026-04-01-research-preview` value -- it's
  universally needed when outcomes is in play, deserves a typed
  name. (Currently would round-trip as `Other`.)

---

## Confirmed correct (no work needed)

- [x] `UserDefineOutcomeEvent` matches `define-outcomes.md` exactly
      (`description`, `rubric`, `max_iterations`, plus echoed
      `outcome_id` + `processed_at`).
- [x] `OutcomeRubric { Text { content } | File { file_id } }`.
- [x] `Agent.callable_agents` shape: `{type:"agent", id, version}`.
- [x] `OutgoingUserEvent::ToolConfirmation.session_thread_id`,
      `OutgoingUserEvent::CustomToolResult.session_thread_id`.
- [x] Thread events: `session.thread_created` (with
      `session_thread_id` + `model`), `session.thread_idle`,
      `agent.thread_message_sent` (with `to_thread_id` + `content`),
      `agent.thread_message_received` (with `from_thread_id` +
      `content`).
- [x] `SessionStatus { Idle, Running, Rescheduling, Terminated }`
      matches docs exactly.
- [x] Endpoint coverage on **all** non-managed-agents modules:
      messages, models, batches, files, skills, user_profiles, all
      admin sub-resources.
- [x] Endpoint coverage on managed-agents **vaults, memory_stores,
      threads, events**.

---

## Audit progress

- [x] managed_agents (sessions, events, agents, threads, outcomes,
       environments, vaults, memory_stores, resources)
- [x] messages
- [x] models
- [x] batches
- [x] files
- [x] admin (organization, invites, users, workspaces,
       workspace_members, api_keys, usage_report, cost_report,
       rate_limits)
- [x] skills
- [x] user_profiles
- [x] beta (BetaHeader)

(Field-level checks were spot-checks; if a high-fidelity audit is
wanted on every module's struct shapes, that's another pass.)

---

## Summary for fix-pass planning

| Priority | Items | Estimated lines |
|---|---|---|
| Critical -- bugs | 1 (beta gate) + 4 (missing endpoints) + 4 (untyped events) + 8 (missing fields/variants) | ~600 net add |
| High -- doc | 1 | ~5 |
| Medium -- enum | 1 | ~10 |

Once these are closed and tests pass, the API surface is
doc-aligned and we can move to the mock-test gap-filling pass and
then the live-test pass.
