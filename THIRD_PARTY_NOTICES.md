# Third-party dependency notice

This repository publishes source code only. It does not vendor `node_modules`,
Tencent SDK source, Tencent connector code, or compiled artifacts containing
those packages. `pnpm install` retrieves dependencies directly from the npm
registry using the integrity-pinned lockfile.

## Tencent packages

The v0.2.0 source release was reviewed against these exact registry packages:

| Package                            | Version | Published metadata | Packaged license file |
| ---------------------------------- | ------- | ------------------ | --------------------- |
| `@tencent-connect/qqbot-nodejs`    | 1.0.4   | `MIT`              | Not present           |
| `@tencent-connect/qqbot-connector` | 1.2.0   | `UNLICENSED`       | Not present           |

The connector's published metadata does not grant redistribution rights, and
the SDK tarball does not carry the copyright and license notice needed for a
reviewed redistribution decision. Their appearance in `package.json` and the
lockfile is not a relicensing of either package under this project's MIT
license.

Do not publish a binary, container, installer, npm bundle, or other artifact
that includes either Tencent package until the upstream owner provides terms
and notices that explicitly permit that distribution. Operators are responsible
for confirming that their direct installation and use comply with Tencent's
current terms.

The optional Windows x64 npm CLI release is intentionally thin: its tarball
contains neither `node_modules` nor Tencent package source, and npm resolves
those dependencies from the registry during installation. This does not grant
redistribution rights for either Tencent package.

Other production dependencies are resolved from `pnpm-lock.yaml`; inspect the
installed package notices and run `pnpm licenses list --prod` before creating a
redistributable artifact.
