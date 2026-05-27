# Contributing to raria

Thanks for improving raria. This guide is for contributors. User support,
security, privacy, release, and integration guidance lives under [`docs/`](docs/README.md).

## Development Setup

```bash
git clone https://github.com/AnInsomniacy/raria.git
cd raria
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Working Rules

raria is a Rust-native download manager. The active modernization tracker lives in [`docs/core-modernization/overview.md`](docs/core-modernization/overview.md). It is the source for scope, deletion policy, dependency ownership, and verification.

Contributions should preserve these rules:

1. Land real native behavior, stronger verification, or an honest documentation correction.
2. Add only focused tests that protect public contracts, persistence, protocol boundaries, security, or confirmed regressions.
3. Do not weaken tests, hide regressions, or advertise unsupported capability.
4. Keep changes inside the intended area unless correctness forces a wider change.
5. Use mature libraries for protocol ownership and keep raria policy small.
6. Delete obsolete public surfaces after useful native coverage exists.

Do not add JSON-RPC, aria2 method names, aria2 option names, Gid-facing public
behavior, aria2 session compatibility, old config syntax, legacy client
adapters, or compatibility-only tests.

## Dependency Policy

Before relying on dependency behavior, confirm that the selected library can support the intended capability. The current high-value dependency set is:

- `librqbit`
- `reqwest`
- `suppaftp`
- `russh` / `russh-sftp`
- `redb`
- `axum`
- `tracing`

Record dependency limits or replacement decisions in `docs/core-modernization/dependency-ledger.csv`.

## Workspace Overview

| Crate | Purpose |
| --- | --- |
| `raria-core` | Job model, engine, scheduler, persistence, config, checksum |
| `raria-range` | Shared segmented-download abstractions and executor |
| `raria-http` | HTTP/HTTPS backend |
| `raria-ftp` | FTP/FTPS backend |
| `raria-sftp` | SFTP backend |
| `raria-metalink` | Metalink parser and normalizer |
| `raria-bt` | BitTorrent service integration |
| `raria-rpc` | native HTTP JSON API and WebSocket event stream |
| `raria-cli` | CLI and daemon integration |

## Pull Request Expectations

1. Say which native capability, checkpoint, or documentation correction the change addresses.
2. Add or tighten tests first when behavior changes.
3. Keep the diff inside the declared write scope whenever possible.
4. Run the verification commands that match the scope and report the actual result.
5. Update docs whenever capability claims, dependency limits, or operational guidance change.

Use the pull request template. PR titles should be concise Conventional Commit
style, such as `fix: preserve task state during restart` or `docs: add native
integration guide`.

For bug fixes and features, open or reference an issue first unless the change
is a small maintainer-directed correction. The issue templates are tuned for
raria-native evidence and should not be bypassed for compatibility requests.

## Verification Expectations

- Do not claim tests pass without fresh command output.
- Do not claim late-stage closure from documentation alone.
- If a path is only covered at unit or API level, say so.
- If a gap is blocked upstream or by design, keep it explicit in the modernization tracker.

Public-network downloads are not default verification. Prefer local fixtures,
focused unit tests, native API contract tests, and protocol smoke tests that do
not depend on external availability. Keep raw logs, generated binaries, partial
downloads, and session stores under ignored `var/`.

## Issue Reports

Use the GitHub issue forms for reproducible bugs, crashes, feature requests,
and build or packaging issues. Include the exact raria version, platform,
command or API request, relevant `raria.toml` settings, and concise logs with
secrets redacted.

Do not upload bearer tokens, cookies, passwords, SSH keys, private downloads,
or private session databases unless a maintainer explicitly asks for a sanitized
artifact through a private channel.

## Release Contributions

`Cargo.toml` under `[workspace.package]` is the version source of truth. Use
`./scripts/bump-version.sh <major.minor.patch>` for version changes and
`./scripts/release.sh` for local release verification and tag creation. See
[`docs/RELEASE.md`](docs/RELEASE.md) and
[`docs/RELEASE_INTEGRITY.md`](docs/RELEASE_INTEGRITY.md).

## License

By contributing, you agree that your contributions will be licensed under Apache 2.0.
