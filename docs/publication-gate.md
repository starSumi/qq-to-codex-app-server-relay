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
- [ ] The source release and optional npm artifact contain no bundled Tencent
      package source or `node_modules`; npm resolves those dependencies at
      install time and their upstream terms remain the operator's boundary.
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

The v0.2.1 release record was completed on 2026-08-23. The public npm package
`qq-to-codex-app-server-relay@0.2.1` was published with public access, the
registry `latest` tag resolved to `0.2.1`, and a clean global installation
reported `qq-to-codex-app-server-relay --version` as `0.2.1`. The published
Windows x64 tarball contained 35 files, with a 2.2 MB compressed package and a
4.7 MB unpacked payload; no runtime credentials or `node_modules` were included.

The local human smoke test also confirmed a QQ inbound message, Codex command
execution, thread identity, durable reply delivery, and QQ API `200 OK`. These
checks prove the release and integration path, but do not approve the default
profile for production deployment.

## npm publisher authentication

Publishing is an account-level operation and is separate from QQ or Codex
runtime credentials. The supported routes are:

- A user-level npm token configured by npm, followed by
  `npm publish --registry=https://registry.npmjs.org --access public`.
- An interactive WebAuthn security key or Windows Hello challenge using
  `scripts/publish-npm-with-totp.ps1 -SecurityKey -Publish`.
- A TOTP setup key or `otpauth://` URI using the helper's default `-Publish`
  mode.

Recovery codes are not TOTP setup keys, and an npm token cannot be converted
into a six-digit TOTP value. Never commit `.npmrc`, pass a token on the command
line, or store any publisher secret in the repository. Verify a release with
`npm view` and `npm whoami`; neither command requires printing the secret.

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
