# Changelog

All notable changes to `claude-api-test` are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and uses [Semantic Versioning](https://semver.org/).

## [0.5.4](https://github.com/joshrotenberg/claude-api/compare/claude-api-test-v0.5.3...claude-api-test-v0.5.4) - 2026-05-02

### Other

- updated the following local packages: claude-api

## [0.5.3](https://github.com/joshrotenberg/claude-api/compare/claude-api-test-v0.5.2...claude-api-test-v0.5.3) - 2026-05-02

### Added

- *(live-tests)* add test stubs for user-profiles, skills write, batches completion, conversation, runner ([#36](https://github.com/joshrotenberg/claude-api/pull/36))
- *(test)* SSE cassette recording and replay support ([#38](https://github.com/joshrotenberg/claude-api/pull/38))
- *(models)* type-promote ModelInfo.capabilities ([#27](https://github.com/joshrotenberg/claude-api/pull/27))

### Other

- *(streaming)* record live cassette for messages.create_stream ([#39](https://github.com/joshrotenberg/claude-api/pull/39))

## [0.5.2](https://github.com/joshrotenberg/claude-api/compare/claude-api-test-v0.5.1...claude-api-test-v0.5.2) - 2026-05-01

### Other

- updated the following local packages: claude-api

## [0.5.1](https://github.com/joshrotenberg/claude-api/compare/claude-api-test-v0.5.0...claude-api-test-v0.5.1) - 2026-05-01

### Other

- updated the following local packages: claude-api

## [0.5.0] -- 2026-05-01

Workspace version bump alongside `claude-api` 0.5.0.

### Changed

- `Recorder` now truncates the cassette file on start instead of
  appending. Each recording run produces a fresh cassette;
  accumulating across runs needs to be done by the caller.

[0.5.0]: https://github.com/joshrotenberg/claude-api/releases/tag/claude-api-test-v0.5.0
