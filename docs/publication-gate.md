# Public source and deployment gates

Public source publication and production deployment are separate decisions.
Passing the source gate permits publishing reviewed code. It does not approve a
bot, credential, host, Codex home, workspace, or external integration for
production use.

## Public source gate

Run these checks against the exact tree copied into a fresh public Git history:

- [ ] `pnpm install --frozen-lockfile --ignore-scripts`
- [ ] `pnpm check`
- [ ] `pnpm test`
- [ ] `pnpm build`
- [ ] Production dependency audits report no high or critical advisory.
- [ ] The release is source-only: no artifact contains either Tencent package;
      their missing distributable notices remain documented.
- [ ] Current files and reachable public Git history contain no credential,
      private identifier, local absolute path, database, log, runtime artifact,
      or private author email.
- [ ] QR onboarding writes only to the ignored credential path outside the Codex
      workspace; AppID and AppSecret never enter child environments or logs.
- [ ] QQ SDK debug is off by default; its redaction fixtures cover gateway
      payloads, HTTP bodies, resource identifiers, trace identifiers, secrets,
      and local paths.
- [ ] Safe mode rejects a missing or wildcard sender allowlist; dangerous mode
      emits an explicit remote-execution warning.
- [ ] GitHub Actions use commit-pinned third-party actions and least-privilege
      permissions.
- [ ] The public root commit uses a GitHub noreply identity and is created from
      the audited tree rather than private repository history.
- [ ] Public CI passes before a release tag is published.

The v0.2.0 release record is completed during publication. The local human smoke
test confirmed a QQ inbound message, Codex command execution, thread identity,
durable reply delivery, and QQ API `200 OK`. This test did not approve the
default profile for production deployment.

## Production deployment gate

Use a dedicated test bot, disposable workspace, dedicated `CODEX_HOME`, and
credentials that will be rotated after testing. Start with the lock-down
profile:

```powershell
$env:RELAY_AUTH_MODE = "safe"
$env:QQBOT_ALLOW_FROM = "<test sender openid>"
$env:CODEX_SANDBOX = "read-only"
$env:CODEX_NETWORK_ACCESS = "false"
$env:CODEX_ALLOW_EXTERNAL_TOOLS = "false"
```

Before approving a real deployment, record all of the following against its
exact bot, host, Codex version, configuration, and candidate commit:

- [ ] An allowlisted account receives the correct reply.
- [ ] A non-allowlisted account receives no reply and creates no Codex turn.
- [ ] A bot-authored event does not loop.
- [ ] `/reset` starts a new Codex thread for that target.
- [ ] Oversized and rate-limited requests fail without internal disclosure.
- [ ] Restart during a request produces the documented recovery response.
- [ ] A pending outbox entry replays after restart without losing the reply.
- [ ] Runtime logs contain no raw QQ identifiers, bodies, credentials, or paths.
- [ ] SQLite contains no inbound plaintext and nulls delivered output text.
- [ ] Credential file and database ACLs restrict access to the relay account.
- [ ] The QQ credential used for the canary is rotated after testing.

The default profile requires a separate, explicit risk acceptance because it
combines open sender admission, `danger-full-access`, network access, and the
dedicated Codex home's configured external tools. Inventory and canary every
MCP server, hook, plugin, app, browser/computer tool, remote-control path, and
credential before enabling that profile outside a private test account.

## GitHub release controls

Before marking a public tag stable:

- [ ] Enable private vulnerability reporting, secret scanning, dependency
      alerts, and protected-branch or ruleset controls supported by the plan.
- [ ] Require the public CI and security workflows on the release commit.
- [ ] Confirm the repository description and documentation state that public
      source availability is not production approval.

An old private repository can retain provider-side objects, caches, or prior
clones even after a history rewrite. The public project therefore starts from a
fresh root commit containing only the audited release tree; the private source
repository remains a separate archive.
