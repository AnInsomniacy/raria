# Contributing to raria

Thanks for helping improve raria.

## Development Setup

```bash
git clone https://github.com/AnInsomniacy/raria.git
cd raria
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Working Rules

raria currently follows the practical maturity contract described in [`docs/practical-maturity.md`](docs/practical-maturity.md). Contributions should preserve these rules:

1. **Capability-first delivery** — each change should land real behavior, better verification, or an honest documentation update.
2. **TDD is required** — add or tighten tests first when changing behavior.
3. **No fake green** — do not weaken tests, hide regressions, or claim unsupported capability.
4. **Write-scope discipline** — stay inside the step's allowed crates/files unless correctness clearly forces a broader change.
5. **Facade honesty** — aria2-style responses may default or omit unstable fields, but must not distort internal truth.

## Hard Governance Gates

Every meaningful change should respect the three active hard gates:

### 1. Stop-line grading

If a dependency or architectural limit prevents parity, record it honestly instead of papering it over. Use these grades:

- `core-blocking`
- `advanced-but-acceptable`
- `migration-only`

### 2. Dependency viability audit

Before leaning on dependency behavior, confirm the dependency can actually support the intended capability. The current high-value dependency set is:

- `librqbit`
- `reqwest`
- `suppaftp`
- `russh` / `russh-sftp`
- `redb`
- `jsonrpsee`

### 3. Write-scope / crate-boundary discipline

Each roadmap step has:

- a **primary write** crate
- optional **supporting write** crates
- explicit **forbidden** crates

If you need to cross those boundaries, document why the wider change is unavoidable.

## Workspace Overview

| Crate | Purpose |
| --- | --- |
| `raria-core` | Job model, engine, scheduler, persistence, config, checksum |
| `raria-range` | Shared segmented-download abstractions and executor |
| `raria-http` | HTTP/HTTPS backend |
| `raria-ftp` | FTP/FTPS backend |
| `raria-sftp` | SFTP backend |
| `raria-metalink` | Metalink parser / normalizer |
| `raria-bt` | BitTorrent service integration |
| `raria-rpc` | aria2-style JSON-RPC server / facade |
| `raria-cli` | CLI and daemon integration |

## Pull Request Expectations

1. Identify which practical-maturity step, stop-line, or documentation correction your change addresses.
2. Add tests first when behavior changes.
3. Keep the diff inside the declared write scope whenever possible.
4. Run the relevant verification commands before opening the PR.
5. Update docs when capability claims or operational guidance change.

## Verification Checklist

Before submitting a change, run the checks that match your scope:

```bash
cargo test --workspace
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For narrower changes, include focused crate/test evidence as well.

## License

By contributing, you agree that your contributions will be licensed under Apache 2.0.
