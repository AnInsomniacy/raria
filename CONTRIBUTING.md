# Contributing to raria

Thanks for improving raria.

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

## Verification Expectations

- Do not claim tests pass without fresh command output.
- Do not claim late-stage closure from documentation alone.
- If a path is only covered at unit or API level, say so.
- If a gap is blocked upstream or by design, keep it explicit in the modernization tracker.

## License

By contributing, you agree that your contributions will be licensed under Apache 2.0.
