# Contributing

## Development contract

- Preserve the Rust-first ownership boundary. Authorization, persistence,
  limits, recovery, Codex lifecycle, and wire semantics belong in Rust.
- Treat `crates/relay-protocol/src/lib.rs` as the relay protocol SSOT.
- Never hand-edit `schemas/relay-wire.schema.json` or
  `apps/qq-adapter/src/generated/relay-wire.ts`.
- Keep the TypeScript adapter limited to Tencent SDK integration,
  normalization, runtime validation, process supervision, and QQ delivery.
- Never add credentials, raw QQ identifiers, message bodies, databases, Codex
  homes, or machine-specific absolute paths to fixtures or logs.
- Keep remote approval fail-closed. Adding an approval UI requires a separate
  threat model and protocol revision.
- Do not publish bundled dependencies or compiled release artifacts until every
  included package has a reviewed distributable license and notice. The pinned
  Tencent connector is currently source-install only; see
  `THIRD_PARTY_NOTICES.md`.

## Setup and validation

Use the pinned Volta, pnpm, and Rust toolchains.

```powershell
pnpm install --frozen-lockfile --ignore-scripts
pnpm check
pnpm test
pnpm build
```

When Rust protocol types change, regenerate both derived artifacts and commit
them with the source change:

```powershell
pnpm schema:generate
pnpm check
```

Pull requests must explain the trust boundary affected, failure and recovery
behavior, data lifecycle impact, compatibility implications, validation, and
rollback. Dependency updates must retain exact lockfiles and pass the security
workflow.

## npm release documentation

The npm artifact is currently a Windows x64 convenience distribution. Before a
release, run the complete source gate and confirm the package contents do not
include `node_modules`, Tencent SDK source, credentials, databases, Codex homes,
or local paths. Publish only from the public checkout; the private development
checkout is intentionally rejected by the helper.

Use [`docs/npm-cli.md`](docs/npm-cli.md) for the install and verification
workflow. Publishing authentication is account-owned: use the configured npm
token, a WebAuthn security key, or a real TOTP setup key. Recovery codes are not
TOTP input. Never put any of these values in Git, a shell command, or a release
artifact.
