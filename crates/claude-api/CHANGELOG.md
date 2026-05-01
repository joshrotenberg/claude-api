# Changelog

All notable changes to this project will be documented in this file.

## [0.5.1] - 2026-05-01

### Documentation

- Rewrite README + crate-level lib.rs + beef up thin module headers 

### Miscellaneous

- Regenerate changelog 
- Release v0.5.1 

## [0.5.0] - 2026-05-01

### Bug Fixes

- *(managed-agents)* Close spec-diff chunk 1 (Session/Agent/Environment)
- *(managed-agents)* Close spec-diff chunk 2 (memory subtree)
- *(managed-agents)* Close spec-diff chunk 3 (Credential restructure)
- Close spec-diff chunk 4 (top-level resources)
- *(managed-agents)* Close spec-diff chunk 5 (events + resources)

### Features

- Claude-api v0.2 -- type-safe Rust client for the Anthropic API
- Claude-api v0.3 -- typed citations, compaction, batches, files, typed built-in tools
- Claude-api v0.4 -- agent-loop guardrails, streaming callbacks, dry_run, cost preview, derive(Tool)
- Claude-api v0.4.1 -- approval gates, record/replay, count-tokens cache, pricing.toml
- *(claude-api-test)* Live-recording proxy for cassette capture
- Claude-api v0.4.2 -- bedrock auth, agent loop checkpoints, prompt-cache aliases
- *(managed-agents)* Preview module + Sessions CRUD + events typed surface
- *(managed-agents)* SSE streaming + vaults + memory stores
- *(managed-agents)* Session resources + agents.create + environments CRUD; bump 0.4.3
- *(managed-agents)* Expand Agents to full CRUD per the published spec
- *(managed-agents)* Multi-agent threads + callable_agents
- *(admin)* Full Admin API coverage (27 endpoints); bump 0.4.4
- *(skills)* Full Skills API coverage (8 endpoints); bump 0.4.5
- Typed BetaHeader + full User Profiles API; bump 0.4.6
- Restructure to crates/ layout + workspace deps + edition 2024 
- Release-plz config + workflow 

### Miscellaneous

- Align repository URL with claude-api repo name

### Testing

- Close blocking-feature + managed-agents coverage gaps
- Fill mock-test gaps for new endpoints + types
- Live-API smoke tests with record-or-replay harness
- Record tier 1 live-API cassettes + add inference_geo to Usage
- Tier 2 live cassettes + recorder truncate fix
- Tier 3 live cassettes + Agent.description/system nullability fix
- Tier 4 live cassettes for admin (read-only, 6 endpoints)

### Audit

- Spec-diff tooling + drift reports

### Release

- Claude-api v0.5.0 


