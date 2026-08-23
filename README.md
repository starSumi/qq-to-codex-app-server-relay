# QQ to Codex app-server relay

A Rust-first, fail-closed relay that connects Tencent QQ Bot directly to Codex
`app-server`. It does not import, launch, or depend on OpenClaw.

> [!WARNING]
> The default profile deliberately exposes local Codex command, file, network,
> and configured external-tool capabilities to every non-bot sender. Publishing
> this source does not approve a production deployment. Read the
> [security model](docs/security-model.md) and select the explicit lock-down
> settings before exposing a bot outside a private test account.

```text
Tencent QQ Bot gateway
  @tencent-connect/qqbot-nodejs
            |
            | normalized, runtime-validated messages
            v
TypeScript adapter -- JSONL + schema hash --> Rust relay core
                                                |
                                                | app-server JSONL over stdio
                                                v
                                         codex app-server
```

The TypeScript layer owns only the official Tencent SDK integration. Rust owns
authorization, validation, rate and concurrency limits, thread routing, Codex
process isolation, persistence, recovery, and the wire protocol.

## Security defaults

- QQ credential onboarding and sender authorization are independent. With no QQ
  credential environment variables, the first start uses the official Tencent
  QR connector and persists the result in an ignored local file outside the
  Codex workspace.
- `RELAY_AUTH_MODE=dangerous` is the sender-policy default: every non-bot QQ
  sender can trigger a Codex turn and startup emits a warning. QR binding proves
  control of the bot credential; it is not a sender identity boundary.
- `RELAY_AUTH_MODE=safe` is the explicit privacy mode and requires a non-empty
  `QQBOT_ALLOW_FROM` sender allowlist. A wildcard is rejected in this mode.
- Codex approval policy is fixed to `never`; the relay has no remote approval UI.
- The default sandbox is Codex `danger-full-access`, so local commands and file
  work can execute without the Windows sandbox runner; `read-only` and
  `workspace-write` remain explicit constrained choices.
- Sandbox network access is enabled by default; set `CODEX_NETWORK_ACCESS=false`
  for the lock-down profile. Tencent, the model provider, and explicitly enabled
  external tools remain separate network trust boundaries.
- MCP servers, lifecycle hooks, plugins, Codex apps, browser/computer tools,
  web search, remote control, code mode, and multi-agent execution are enabled
  by default from the dedicated `CODEX_HOME` configuration. Set
  `CODEX_ALLOW_EXTERNAL_TOOLS=false` for the lock-down profile. In the default
  dangerous sender mode this is remote execution for every non-bot QQ sender;
  keep the bot private or use the explicit sender allowlist.
- QQ credentials and unrelated parent secrets are excluded from the Rust and
  Codex child-process environments.
- The SQLite state database and `CODEX_HOME` must resolve outside the Codex
  workspace.
- QQ SDK debug logging is disabled by default. If explicitly enabled, gateway
  payloads, HTTP bodies, raw identifiers, trace identifiers, credentials, and
  local paths are redacted; Rust logs use stable pseudonyms.
- The adapter and core reject unknown protocol fields and refuse to activate if
  their generated JSON Schema hashes differ.

Read [the security model](docs/security-model.md) before the first run.

## Requirements

- Codex CLI with `app-server` support. The current compatibility baseline is
  Codex CLI `0.148.0`; re-run the full test gate after any upgrade.
- Rust `1.94.0`, pinned by `rust-toolchain.toml`.
- Node.js `24.15.0` and pnpm `10.26.0`, pinned through Volta.
- A mobile QQ account able to complete the official Tencent QQ Bot QR binding.

The GitHub release is source-first. The optional npm artifact is a thin
Windows x64 launcher and does not bundle `node_modules` or Tencent package
source. The Tencent connector is published as `UNLICENSED`, and neither
pinned Tencent package includes a distributable license file. Do not
redistribute a binary or bundle containing them; see [the third-party
notice](THIRD_PARTY_NOTICES.md).

## Build

```powershell
pnpm install --frozen-lockfile --ignore-scripts
pnpm check
pnpm test
pnpm build
```

`pnpm schema:generate` is the only supported way to refresh the checked-in
Schema and TypeScript contract after changing Rust protocol types.

## npm CLI (Windows x64)

The public npm artifact is a thin Windows x64 launcher around the same
Rust-first relay. It contains the compiled relay core and TypeScript adapter
build, but never bundles `node_modules`, Tencent SDK source, credentials, or
runtime state. Install it with:

```powershell
npm install --global qq-to-codex-app-server-relay
qq-to-codex-app-server-relay
```

The launcher stores credentials, SQLite state, and the default workspace under
`%LOCALAPPDATA%\\qq-to-codex-app-server-relay` unless the corresponding
environment variables are explicitly set. The package is currently restricted
to `win32/x64`; other platforms must use the source release and build locally.
Review [the third-party notice](THIRD_PARTY_NOTICES.md) before distributing or
embedding the package.

### npm publishing helper

Maintainers can publish the current release without copying an OTP into a
shell command. The helper reads the TOTP secret through hidden local input,
generates the current code in memory, and passes it directly to npm:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-npm-with-totp.ps1 -Publish
```

Use `-Build` when the release artifacts need rebuilding first. The secret is
not written to disk or printed. A secret already held in an environment
variable can be selected with `-SecretEnvironmentVariable NAME`; a local
secret file can be selected with `-SecretFile PATH`.

The input must be the authenticator setup key (Base32) or an `otpauth://` URI.
npm recovery codes are not TOTP setup keys and cannot be used to calculate the
six-digit publish OTP; use them only in npm's web recovery flow.

This npm account uses WebAuthn security keys, so use this mode instead. It does
not ask for a secret or pass `--otp`; npm opens the security-key challenge and
Windows Hello or the browser completes it:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-npm-with-totp.ps1 -SecurityKey -Publish
```

## Configure

After the build, the default first run needs no QQ credential variables:

```powershell
pnpm start
```

Scan the terminal QR code with mobile QQ. The adapter stores the returned bot
credential at `state/qqbot-credentials.json`; later starts reuse it without
rescanning. `state/` is ignored by Git, but the file contains a plaintext secret:
restrict its filesystem ACL, never copy it into the Codex workspace, and revoke
the bot credential if the file is exposed.

Startup reports each completed boundary without printing identifiers or
secrets: credential source or QR completion, Codex app-server readiness, QQ
gateway connection, and relay activation. `Codex app-server ready` is emitted
only after native app-server initialization checks pass.

The defaults use `~/.codex` as `CODEX_HOME`, create the ignored `./workspace` as
the working directory, keep network access and optional external integrations
enabled, and use the deliberately open `dangerous` sender mode. The default
`danger-full-access` policy is not confined to that workspace and can reach
other files available to the relay account. Keep the bot private and run it as
a dedicated low-privilege OS account with a disposable workspace.

For the explicit safe sender mode:

```powershell
$env:RELAY_AUTH_MODE = "safe"
$env:QQBOT_ALLOW_FROM = "<allowed QQ user openid>"
pnpm start
```

For unattended deployment, provide `QQBOT_APP_ID` and `QQBOT_APP_SECRET`
together through the service manager. That pair takes precedence over the local
credential file and QR flow. `QQBOT_CREDENTIALS_FILE`, `CODEX_HOME`,
`CODEX_WORKSPACE`, and `RELAY_DB` can override their local defaults. The relay
intentionally does not load `.env` files.

The `dangerous` mode is not a pairing mechanism: the first and every subsequent
non-bot sender is admitted. Use `safe` for a personal relay or any deployment
reachable outside the operator's trust boundary.

See [`.env.example`](.env.example) for bounded optional settings. Do not place
real credentials in that file or commit them anywhere.

`CODEX_ALLOW_EXTERNAL_TOOLS=true` keeps the dedicated Codex home's configured
MCP, hooks, plugins,
apps, browsing, remote-control, code-mode, memory/goal tools, JavaScript REPL,
and multi-agent surfaces. That is a separate deployment profile and requires
its own threat model and test gate.

Each C2C or group target maps to one durable Codex thread. Send `/reset` to
discard the current target's binding and start a new thread on the next message.
Only text messages and text replies are supported.

Do not run this relay and another QQ gateway with the same bot credentials at
the same time. Competing sessions can cause duplicate or lost delivery.

## Data and delivery

The relay database stores message and target identifiers, a content hash,
thread bindings, delivery state, and pending reply text. It does not store
inbound message bodies. Delivered reply text is nulled immediately after the QQ
acknowledgement. Completed metadata and stale thread bindings expire after 30
days by default; pending outbox entries are retained for recovery.

Delivery is at least once. A process failure after QQ accepts a reply but before
the acknowledgement is committed can duplicate that reply after restart. Run
exactly one relay instance per database and bot credential set.

Codex owns its own thread history under `CODEX_HOME`; relay retention does not
delete it. See [operations](docs/operations.md) for backup, recovery, and
retention details.

## Repository map

| Path                             | Owner                                                   |
| -------------------------------- | ------------------------------------------------------- |
| `crates/relay-protocol`          | Rust wire types and JSON Schema generation              |
| `schemas/relay-wire.schema.json` | Generated protocol artifact                             |
| `crates/relay-core`              | Policy, persistence, recovery, and Codex lifecycle      |
| `apps/qq-adapter`                | Thin Tencent SDK boundary and runtime Schema validation |
| `docs/architecture.md`           | Lifecycle, trust boundaries, and SSOT chain             |

## Project status

The implementation is intentionally narrow: text input, `/reset`, durable
thread routing, bounded execution, restart recovery, and text output. It does
not provide attachments, a remote approval interface, multi-replica HA,
end-to-end encryption, or database encryption at rest.

The public repository is a source release, not a production-deployment
certification. Review [CONTRIBUTING.md](CONTRIBUTING.md),
[SECURITY.md](SECURITY.md), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md),
and the separate
[source and deployment gates](docs/publication-gate.md) before operating it.
