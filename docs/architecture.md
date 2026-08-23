# Architecture

## Connected lifecycle

```text
QQ event
  -> TypeScript SDK callback
  -> normalize + JSON Schema validation
  -> Rust authorization mode + size/rate/queue checks
  -> durable inbox claim
  -> resume or start target's Codex thread
  -> run one bounded Codex turn
  -> durable outbox entry
  -> TypeScript QQ send
  -> delivery acknowledgement
  -> erase delivered reply text
```

The adapter does not decide authorization, sandbox policy, thread ownership, or
recovery. Those decisions remain in the Rust core so every inbound transport
would receive identical semantics.

## Distribution and operator lifecycle

The published Windows package adds only a packaging boundary around the same
runtime:

```text
npm global shim
  -> bin/qq-to-codex-app-server-relay.mjs
  -> apps/qq-adapter/dist/main.js
  -> target/release/relay-core.exe
```

The shim checks `win32/x64`, resolves the per-user state root, derives default
paths for the credential file, SQLite database, and workspace, then launches
the adapter. It does not replace the app-server protocol, introduce OpenClaw,
or make authorization decisions. Source builds invoke the adapter directly and
retain the same Rust/TypeScript ownership boundaries.

The operator lifecycle is:

```text
install and verify version
  -> start one relay instance
  -> QQ QR binding or credential-file load
  -> Codex app-server readiness
  -> QQ gateway connection
  -> relay activation
  -> inbound QQ message -> durable Codex turn -> QQ delivery
```

The readiness messages correspond to actual process boundaries. A successful
QQ gateway connection does not prove that a Codex turn or model provider is
available; those failures retain a relay reference for correlation.

## QQ credential onboarding

The TypeScript adapter resolves one QQ Bot credential source in strict priority
order:

```text
paired QQBOT_APP_ID + QQBOT_APP_SECRET
  -> versioned QQBOT_CREDENTIALS_FILE
  -> official @tencent-connect/qqbot-connector QR binding
  -> atomic local credential-file write
```

The default file is `state/qqbot-credentials.json`. It must resolve outside the
Codex workspace, is never forwarded to Rust or Codex, and is reused on later
starts. QR onboarding establishes bot credentials only; it does not authorize
message senders. The resolver exposes credential-source, QR-required,
QR-success, and QR-failure status callbacks; the adapter maps them to
secret-free operator messages.

## Authorization modes

`RELAY_AUTH_MODE` is parsed by the Rust core and defaults to `dangerous`:

- `dangerous` admits every non-bot inbound sender and logs a startup warning. It
  changes only sender admission; Codex sandbox, approval, network, credential
  onboarding, and external-tool policies remain independent settings.
- `safe` requires explicit comma-separated QQ user OpenIDs in
  `QQBOT_ALLOW_FROM` and rejects `*`.

The adapter passes these policy variables through its restricted core
environment. It never makes the authorization decision itself.

## Ownership boundaries

| Surface                 | Owner              | Contract                           |
| ----------------------- | ------------------ | ---------------------------------- |
| QQ gateway and send API | TypeScript adapter | Official Tencent SDK               |
| Adapter/core protocol   | `relay-protocol`   | Versioned JSONL messages           |
| Policy and state        | `relay-core`       | Rust types and SQLite transactions |
| Model runtime           | Codex              | `app-server --stdio` protocol      |

Tencent SDK types and Codex app-server messages are external contracts. They
are validated at their boundaries but are not redefined as project-owned SSOTs.

## Protocol SSOT

```text
Rust enums and structs
  crates/relay-protocol/src/lib.rs
             |
             v
generated JSON Schema
  schemas/relay-wire.schema.json
             |
             v
generated TypeScript declarations
  apps/qq-adapter/src/generated/relay-wire.ts
             |
             v
Ajv validation at every JSONL ingress
```

Both processes compute the SHA-256 digest of the checked-in Schema. The core
does not accept traffic until `hello`, `ready`, and `activate` prove protocol
version and Schema equality. Generated artifacts are checked for drift in CI.

## Concurrency and ordering

The Rust core bounds the global queue and active turn count. A per-target lock
serializes turns for one chat while permitting independent targets to progress
concurrently. SQLite inbox claims deduplicate platform message IDs and reject a
message ID reused with different content or target data.

This deployment is single-writer by design. Multiple relay processes sharing a
database or bot credentials are unsupported.

## Failure and recovery

- Inbound claims are durable before a Codex turn starts.
- A restart converts interrupted claims to explicit replayable error messages.
- Replies are written to a durable outbox before being sent to QQ.
- Pending outbox records replay after adapter/core restart.
- A delivery acknowledgement marks the record delivered and removes its text.
- A failure between remote acceptance and local acknowledgement can duplicate a
  reply. This is the unavoidable at-least-once boundary without a Tencent-side
  idempotency contract.

The Codex child is killed with the core, requests and turns have deadlines, and
all supported server-initiated approval requests are declined. The default
profile enables the user's dedicated Codex tool surfaces and uses
`dangerFullAccess`; `CODEX_ALLOW_EXTERNAL_TOOLS=false` applies process-only
overrides and verifies effective MCP, feature, web-search, and hook state before
activation. This closed-loop verification, rather than strict parsing of the
user's entire Codex configuration, is the readiness contract.
