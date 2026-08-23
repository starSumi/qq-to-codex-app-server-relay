# npm CLI Guide

This guide covers the published `qq-to-codex-app-server-relay` package. The
package is a Windows x64 distribution of the same Rust-first relay in this
repository. It is a launcher and compiled runtime, not a different protocol or
an OpenClaw integration.

## Install and verify

The published artifact requires Windows x64, Node.js `>=24.15.0`, and a Codex
CLI installation that provides `app-server` support.

```powershell
npm install --global qq-to-codex-app-server-relay@0.2.1
qq-to-codex-app-server-relay --version
npm view qq-to-codex-app-server-relay version dist-tags.latest --registry=https://registry.npmjs.org
```

The second command must print `0.2.1`. The registry query is a read-only check
that the installed release is also the current `latest` tag. `--help` prints
the supported command and environment variable surface.

The npm artifact is currently platform-gated. On Linux, macOS, or a non-x64
Windows host, use the source checkout and the Rust/pnpm build described in the
main [README](../README.md).

## First start

Start the relay from any PowerShell prompt:

```powershell
qq-to-codex-app-server-relay
```

When no `QQBOT_APP_ID` and `QQBOT_APP_SECRET` are present and no credential
file exists, the official Tencent QQ QR binding flow starts. Scan the QR code
with mobile QQ and wait for the adapter to report credential completion. Later
starts reuse the saved credential and do not require another scan.

A healthy startup reaches these boundaries in order:

```text
QQ credential loaded or QR binding completed
Codex app-server ready
QQ gateway connected
relay active; waiting for QQ messages
```

These messages are readiness checkpoints, not model responses. A QQ message is
accepted only after the final `relay active` checkpoint. Press `Ctrl+C` once to
stop the single relay process cleanly.

## State and configuration

The launcher derives a per-user state root so an npm installation does not put
runtime data in the package directory:

| Variable                 | Default in npm mode                            | Purpose                                |
| ------------------------ | ---------------------------------------------- | -------------------------------------- |
| `RELAY_STATE_DIR`        | `%LOCALAPPDATA%\\qq-to-codex-app-server-relay` | Root for local relay state             |
| `QQBOT_CREDENTIALS_FILE` | `<state root>\\qqbot-credentials.json`         | QQ AppID/AppSecret file                |
| `RELAY_DB`               | `<state root>\\relay.sqlite3`                  | Durable inbox/outbox database          |
| `CODEX_WORKSPACE`        | `<state root>\\workspace`                      | Default Codex working directory        |
| `CODEX_HOME`             | `%USERPROFILE%\\.codex`                        | Codex configuration and thread history |

Every path can be overridden with an absolute path. Keep the credential file,
database, and Codex home outside synchronized folders and public repositories.
The credential file contains a plaintext bot secret; restrict its Windows ACL.

The default sender policy is intentionally `dangerous`, which admits every
non-bot sender. For a personal or remotely reachable deployment, set the
explicit allowlist before starting:

```powershell
$env:RELAY_AUTH_MODE = "safe"
$env:QQBOT_ALLOW_FROM = "<allowed QQ user openid>"
qq-to-codex-app-server-relay
```

The default execution profile also enables `danger-full-access`, network access,
and configured external Codex tools. Use the lock-down settings documented in
the [security model](security-model.md) when the process is not confined to a
private test account.

## Upgrade, uninstall, and diagnostics

```powershell
npm install --global qq-to-codex-app-server-relay@0.2.1
npm view qq-to-codex-app-server-relay version dist-tags.latest --registry=https://registry.npmjs.org
npm uninstall --global qq-to-codex-app-server-relay
```

Uninstalling the package does not remove the state root, Codex history, QQ
credential, or SQLite database. Remove those files only after exporting or
rotating the credentials and confirming that no relay process is running.

Useful non-secret diagnostics are:

```powershell
npm whoami --registry=https://registry.npmjs.org
qq-to-codex-app-server-relay --help
```

Common failures:

- `This npm release supports Windows x64 only.` means the platform gate failed;
  use the source build on another platform.
- `The TypeScript adapter build` or `relay-core executable` is missing means the
  npm artifact is incomplete; reinstall the exact version.
- A QR or gateway failure is a QQ credential, network, or Tencent account
  problem, not a Codex model response.
- `Codex app-server ready` never appears when the local Codex executable,
  `CODEX_HOME`, or provider configuration cannot start app-server. Inspect the
  redacted local relay log and test Codex locally.
- `The model provider is temporarily unavailable` is an upstream provider
  failure. The reference identifier in the QQ reply is the correlation key; it
  is not a QQ authentication failure.

Do not start a second relay with the same QQ bot credential or database. QQ
gateway sessions and durable delivery are single-writer boundaries.

## Publishing the npm artifact

Only publish from the public source checkout after the full source gate passes:

```powershell
pnpm install --frozen-lockfile --ignore-scripts
pnpm check
pnpm test
pnpm build
npm whoami --registry=https://registry.npmjs.org
npm publish --registry=https://registry.npmjs.org --access public
```

The package version is immutable on npm. Verify the result without exposing a
token:

```powershell
npm view qq-to-codex-app-server-relay@0.2.1 name version --registry=https://registry.npmjs.org
npm view qq-to-codex-app-server-relay version dist-tags.latest --registry=https://registry.npmjs.org
```

The npm credential belongs to the publisher account and is separate from QQ
bot credentials and Codex credentials. A token is normally stored in the
publisher's user-level npm configuration; never commit `.npmrc`, pass a token
on a command line, or put it in an issue, log, or environment file that is
shared with the relay.

This repository also provides a helper for interactive 2FA:

```powershell
# WebAuthn security key or Windows Hello, no OTP secret required
pwsh -NoProfile -ExecutionPolicy Bypass -File .\\scripts\\publish-npm-with-totp.ps1 -SecurityKey -Publish

# TOTP setup key only; recovery codes and npm tokens are not valid here
pwsh -NoProfile -ExecutionPolicy Bypass -File .\\scripts\\publish-npm-with-totp.ps1 -Publish
```

The helper expects prebuilt artifacts unless `-Build` is supplied. WebAuthn
private keys remain in the authenticator or Windows credential manager and are
not exportable as TOTP secrets. Recovery codes are for npm's recovery flow and
cannot be converted into six-digit TOTP values.
