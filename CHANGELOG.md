# Changelog

All notable changes will be documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) after its first
stable release.

## [Unreleased]

## [0.2.0] - 2026-08-23

### Added

- Rust-first relay core with fail-closed Codex app-server integration.
- Rust-owned JSON Schema contract, generated TypeScript types, runtime Ajv
  validation, and a schema-hash activation handshake.
- Explicit `safe` sender allowlist mode and deliberately open `dangerous`
  default mode, plus bounded queues, rate limits, per-target serialization,
  turn timeouts, and output limits.
- Durable SQLite inbox, outbox, thread bindings, restart recovery, delivery
  acknowledgement, content redaction, and retention.
- Thin TypeScript adapter for the official Tencent QQ Bot SDK.
- Pinned pnpm, Volta, Rust, dependency, CI, and security-audit configuration.
- Official QR credential onboarding with ignored local persistence and
  environment-variable support for managed deployment.

### Changed

- Default Codex execution uses `danger-full-access`, network access, and the
  dedicated Codex home's external tools. Every capability has an explicit
  lock-down setting documented for operators.

### Security

- QQ SDK debug output is disabled by default. Explicit debug logs redact gateway
  payloads, HTTP bodies, raw platform identifiers, trace identifiers, local
  paths, and credentials.
- QQ credentials and unrelated parent-process secrets are excluded from Rust
  and Codex child environments.
