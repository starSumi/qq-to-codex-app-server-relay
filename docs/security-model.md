# Security model

## Assets and trust boundaries

Protected assets include QQ bot credentials, authorized user messages, local
files reachable by Codex, Codex credentials and history, persisted routing
metadata, and model output.

The relay crosses four trust boundaries: Tencent's gateway, the TypeScript/Rust
JSONL channel, the Codex app-server child process, and the local filesystem. QQ
sender identifiers are authorization inputs and must not be treated as public
metadata.

## Controls

- The adapter accepts only a complete environment credential pair, a strict
  versioned local credential file, or one official QR result. The local file is
  created atomically outside the Codex workspace with owner-only POSIX mode
  where supported, is ignored by Git, and is never forwarded to Rust or Codex.
- The Rust core defaults to `RELAY_AUTH_MODE=dangerous`; startup emits a warning
  and every non-bot sender is admitted. QR binding and sender authorization are
  independent. `RELAY_AUTH_MODE=safe` is the explicit exact-allowlist mode.
- Bot-authored messages are ignored to prevent reply loops.
- Input/output bytes, queue depth, active turns, per-sender rate, and turn time
  are bounded.
- Chat targets are serialized to prevent concurrent mutations in one Codex
  thread.
- `danger-full-access` is the default so the relay can perform local development
  work and avoid the broken Windows sandbox runner path observed on this host.
  Sandbox network access is also enabled by default through
  `CODEX_NETWORK_ACCESS=true` for constrained policies; set it to `false`
  and use `workspace-write` or `read-only` for a constrained mode.
- External execution surfaces are enabled by default from the dedicated
  `CODEX_HOME`. Set `CODEX_ALLOW_EXTERNAL_TOOLS=false` to apply process-only
  `enabled=false` overrides to each safely addressable MCP server and disable
  hooks/plugins/apps/browser/computer tools, web search, remote control, code
  mode, multi-agent execution, JavaScript REPL, goals/memories, auth elicitation,
  remote plugins, tool suggestions, and the legacy notifier. Because Codex
  configuration tables merge across layers, the lock-down profile checks the
  effective state through native app-server methods and refuses startup if an
  unsafe value survives. It never rewrites the user's Codex configuration.
- `approvalPolicy` is fixed to `never`. Server-initiated command, file, user
  input, MCP elicitation, and permission requests are rejected or declined.
- The TypeScript child environment is an allowlist. The Rust core applies a
  second allowlist before starting Codex, preserving only required system
  variables and a dedicated `CODEX_HOME`.
- Public error messages are stable and do not expose internal paths, command
  lines, upstream payloads, or exception details.
- QQ SDK debug output is disabled by default. The adapter redacts entire gateway
  payloads and HTTP bodies plus raw resource and trace identifiers if debug is
  explicitly enabled. Rust logs omit message bodies and use stable pseudonyms.
- Unknown protocol fields, oversized JSONL records, version drift, and Schema
  drift fail closed.

## Stored data

The relay SQLite database stores raw platform message and target identifiers,
content hashes, status, thread bindings, and pending output text. It never
stores inbound message content. A successful delivery acknowledgement removes
the output text.

The separate QQ Bot credential file stores the AppID and AppSecret in plaintext.
It requires a restrictive filesystem ACL; POSIX mode bits do not guarantee a
restrictive Windows ACL. Environment credentials can avoid this local
persistence for managed deployments.

Delivered metadata, completed inbox claims, and inactive thread bindings are
purged after `RELAY_RETENTION_DAYS` (30 by default). Pending outbox entries and
active processing records are not aged out. Codex stores its own thread history
under `CODEX_HOME`; deleting relay metadata does not delete that history.

## npm distribution and publisher credentials

The npm package is a distribution boundary, not a runtime trust boundary. It
contains the compiled relay core and adapter build, but not `node_modules`, QQ
credentials, Codex credentials, SQLite state, or the publisher's npm token.
The Windows launcher keeps its default relay state under
`%LOCALAPPDATA%\\qq-to-codex-app-server-relay`; the source build uses its
configured state paths.

Publishing credentials are separate from the credentials used by a running
relay. Keep npm authentication in the publisher's user-level npm configuration
or an approved hardware-backed flow. Never commit `.npmrc`, pass an npm token
as a command-line argument, or copy it into `.env.example`, logs, issues, or
release artifacts. Verify identity with `npm whoami` without printing the
credential itself.

WebAuthn security keys and Windows Hello keep the private credential in the
authenticator or OS credential manager; they do not provide an exportable TOTP
setup key. The publish helper's TOTP mode therefore accepts only an
authenticator setup key or `otpauth://` URI. npm recovery codes are a recovery
mechanism, not TOTP input, and npm tokens cannot be used as TOTP secrets.

Rotate or revoke a token or bot credential immediately if it appears in Git,
logs, terminal history, a screenshot, or a shared environment. The package
version and tarball are public; the authentication material used to publish it
must remain private.

The database is not encrypted at rest. Restrict its filesystem ACL, keep it
outside synchronized/public folders and the Codex workspace, and use full-disk
encryption where required.

## Threats not solved here

- A compromised host, administrator, QQ platform, official SDK, Codex binary,
  model provider, or package registry.
- Encryption at rest for the local QQ Bot credential file.
- Identity compromise of an allowlisted QQ account.
- Exposure or abuse of a deployment intentionally running in dangerous open
  access mode; the QR credential bind is an operator acknowledgement, not a
  sender authentication factor.
- Prompt injection in authorized content or files visible to Codex.
- Traffic analysis or metadata exposure at external providers.
- End-to-end encryption between QQ and the model runtime.
- Exactly-once delivery, multi-replica coordination, or disaster recovery.
- Safe remote approval of commands, file changes, permissions, or MCP actions.
- Containment of the default external-tool profile; MCP, hooks, plugins, hosted
  apps, and browser-backed tools are not governed solely by the local command
  sandbox.

Use a dedicated low-privilege OS account or host boundary for higher-risk
deployments. Treat the default `danger-full-access` plus external-tool profile as
remote code execution across files, commands, network paths, credentials, and
external integrations available to the relay account; the configured workspace
is only the starting directory. Treat `CODEX_ALLOW_EXTERNAL_TOOLS=false` as one
part of the lock-down profile, not a substitute for sender authorization or OS
isolation.

## Secret response

If a QQ secret, Codex credential, token, or private payload reaches Git, logs,
an issue, or an artifact, stop the relay, revoke or rotate the credential at its
owner, preserve minimal incident evidence privately, remove the exposure from
the working tree and reachable history, and review provider audit logs. History
rewrites do not guarantee removal from hosting caches or existing clones.
