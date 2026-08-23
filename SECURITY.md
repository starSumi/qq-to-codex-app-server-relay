# Security policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.2.x   | Yes       |
| < 0.2   | No        |

## Reporting a vulnerability

Do not open a public issue for suspected vulnerabilities, leaked credentials,
or privacy incidents. Use GitHub's private vulnerability reporting for this
repository. If that surface is unavailable, contact the repository owner
privately before sharing reproduction details.

Include the affected commit, impact, prerequisites, a minimal reproduction, and
whether real credentials or user data may have been exposed. Do not include
live QQ secrets, access tokens, raw user identifiers, private message content,
or a production state database.

The owner should acknowledge a complete report within seven days. Disclosure
timing is coordinated after impact and remediation are understood.

## Deployment responsibility

This relay grants a remote messaging identity access to a local Codex runtime.
Operators are responsible for a dedicated `CODEX_HOME`, an isolated workspace,
least-privilege filesystem permissions, credential rotation, host hardening,
backups, and compliance with Tencent and OpenAI terms.

The default `danger-full-access` profile is intentionally not a containment
boundary. It also enables configured external Codex tools and admits every
non-bot sender unless the operator selects safe mode. Use a dedicated
low-privilege host or OS account and review
[`docs/security-model.md`](docs/security-model.md) before operation.
