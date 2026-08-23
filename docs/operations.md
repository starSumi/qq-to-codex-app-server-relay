# Operations

## Deployment shape

Run one adapter/core pair for one QQ credential set and one SQLite database.
Use an OS service manager that restarts the adapter when it exits. The adapter
exits when the core or delivery path fails so the durable outbox can replay on a
clean restart.

Keep these locations separate:

- application checkout and binaries;
- dedicated `CODEX_HOME` with restrictive ACLs;
- dedicated Codex workspace;
- relay SQLite state directory outside the workspace.

For unattended deployment, pass credentials through the service manager's
secret/environment facility. For an interactive local deployment, the first
start can bind by QR and persist a local credential file. The application does
not read `.env` files.

## Startup

```powershell
pnpm install --frozen-lockfile --ignore-scripts
pnpm check
pnpm test
pnpm build
pnpm start
```

When neither `QQBOT_APP_ID` nor `QQBOT_APP_SECRET` is set, the adapter first
looks for `QQBOT_CREDENTIALS_FILE` (default
`state/qqbot-credentials.json`). If it is absent, the official Tencent connector
prints a QR code. A successful scan is written atomically and reused. The two
environment variables must be supplied together and override the file and QR
flow.

The adapter emits a status after each completed startup boundary: credential
source or QR persistence, Codex app-server readiness, QQ gateway readiness, and
relay activation. If QR binding succeeds but no later readiness message appears,
the following log entry identifies the failing boundary without exposing the
credential.

The credential file contains the QQ Bot secret in plaintext. Keep it outside
`CODEX_WORKSPACE`, restrict its file and parent-directory ACL to the relay OS
account, exclude it from backups that lack secret controls, and never commit it.
POSIX mode `0600` is applied where supported; Windows operators must verify the
effective NTFS ACL. To rebind, stop the relay, revoke the old bot credential,
then remove or replace the file before restarting. The relay never reads an
OpenClaw credential or session directory.

The default is `CODEX_SANDBOX=danger-full-access`: Codex can edit files and run
local development commands without the Windows sandbox runner. Set
`CODEX_SANDBOX=workspace-write` or `read-only` for a constrained profile.
`CODEX_NETWORK_ACCESS` defaults to `true` and applies to the constrained
`read-only`/`workspace-write` policies; set it to `false` for that lock-down
profile. `danger-full-access` intentionally bypasses this sandbox network
switch. `CODEX_ALLOW_EXTERNAL_TOOLS`
defaults to `true`, restoring the dedicated Codex home's configured MCP,
web-search, hooks, plugins, apps, browser/computer, remote-control, code-mode,
memory/goal, JavaScript REPL, and multi-agent surfaces. Set it to `false` for
the lock-down profile. In dangerous sender mode, the enabled profile must be
treated as remote code execution exposed through the QQ bot. The core does not
use `--strict-config`, because
a shared home may contain app-owned fields that the installed CLI does not
recognize in strict mode. Instead, it applies process-only overrides for every
safe MCP server key found in the user configuration and verifies the
runtime-effective result through app-server. An MCP key that cannot be safely
overridden, an enabled server from another configuration layer, or an enabled
hook fails startup without rewriting the user's configuration.

The independent sender-policy default is `RELAY_AUTH_MODE=dangerous`; it does
not require `QQBOT_ALLOW_FROM`. The safe mode is recommended for a personal or
shared deployment:

```powershell
$env:RELAY_AUTH_MODE = "safe"
$env:QQBOT_ALLOW_FROM = "<allowed QQ user openid>"
```

Use the open mode only with a private test bot, a dedicated low-privilege OS
account, a disposable workspace, and a separately approved host boundary. It
allows every non-bot QQ sender to create Codex turns, so a QR-bound bot that is
discoverable by other users must not use this mode. QR bind and
environment-provided credentials have identical sender-policy semantics;
neither identifies or authorizes senders.

The enabled external-tool profile requires a dedicated threat model and canary.
Inventory every MCP, hook, plugin, app, browser/computer tool, remote-control
path, and its credentials before exposing the bot beyond the operator's private
QQ account. `CODEX_ALLOW_EXTERNAL_TOOLS=false` is the immediate lock-down
switch and requires a relay restart.

## Health and logs

A healthy startup requires the QQ client, Rust process, SQLite database, Codex
app-server initialization, external-tool isolation check, Schema digest
handshake, and adapter activation.
Process existence alone is not a health signal. Treat a fatal protocol event or
adapter exit as unhealthy and restart the pair.

The relay takes an operating-system instance lock beside `RELAY_DB` during
startup. Run exactly one adapter/core pair for a database and QQ credential set;
a second `pnpm start` now fails before it connects to QQ instead of competing
for the same inbox and gateway state. Stop the existing pair with `Ctrl+C`
before starting it from another terminal.

QQ SDK logging defaults to `QQBOT_LOG_LEVEL=info`, which suppresses gateway
payload and HTTP-body debug records. `debug` may be enabled for diagnosis, but
the adapter still redacts payloads, bodies, platform/resource identifiers,
trace identifiers, credentials, and local paths. Restrict log access anyway:
dependency diagnostics and host service managers may add metadata that the
relay does not control.

For a failed turn, correlate the QQ reply's short `Reference` with the local
`Codex turn failed` record. The structured `failure_kind` is deliberately a
redacted class such as `upstream_bad_gateway`, `upstream_unavailable`,
`upstream_rate_limited`, `upstream_auth_failed`, `turn_timeout`, or
`protocol_error`; it does not include provider URLs, request IDs, prompts, or
model output. A provider-class failure is not fixed by rescanning the QQ QR
code. Verify the selected Codex provider/profile separately, then restart the
relay so the new app-server child loads it. Do not edit a shared `CODEX_HOME`
while the relay is running.

When switching an OpenAI-compatible provider, keep the provider's complete
configuration together: `base_url`, `wire_api`, `requires_openai_auth`, and any
`http_headers` such as `x-openai-actor-authorization`. The relay does not strip
or synthesize provider headers; Codex app-server loads them from `CODEX_HOME`.
Validate the exact provider with a real app-server turn before routing QQ
traffic to it.

## Backup and restore

Stop the relay before copying the SQLite database. Copy the main database and
any `-wal` and `-shm` companions together if an unclean stop prevents a normal
checkpoint. Restore only to the same or newer compatible relay version, keep
the file outside the workspace, and validate with a read-only canary account.

Back up `CODEX_HOME` under a separate policy. It contains Codex-owned history
and credentials and has a different lifecycle from relay state.

Treat the QQ Bot credential file as a secret with a separate rotation and backup
policy from the SQLite database. The relay database does not contain that bot
secret.

## Retention and deletion

At startup the core purges delivered outbox metadata, completed inbox claims,
and stale thread bindings older than `RELAY_RETENTION_DAYS`. Pending output is
retained until it is delivered and acknowledged. Lowering retention does not
remove Codex-owned thread history.

For intentional decommissioning, stop the service, revoke the QQ secret, secure
or destroy the relay database and backups according to policy, and separately
remove the dedicated `CODEX_HOME` and workspace if their owners authorize it.

## Upgrade and rollback

1. Stop the relay and take a consistent state backup.
2. Record the current commit, Codex CLI version, and lockfile hashes.
3. Build and run all gates from a clean checkout.
4. Test in `read-only` mode with a non-production bot and workspace.
5. Deploy one instance and inspect startup plus one complete delivery.

Rollback the binaries and lockfiles to the recorded commit. A database whose
`user_version` is newer than the binary is rejected; restore the matching
backup instead of forcing a downgrade.
