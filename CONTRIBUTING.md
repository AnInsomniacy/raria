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

raria follows the progress-adjusted practical maturity contract described in [`docs/practical-maturity.md`](docs/practical-maturity.md). The verification standard for completion claims lives in [`docs/verification-contract.md`](docs/verification-contract.md), and the public BitTorrent parity limits live in [`docs/bt-stop-lines.md`](docs/bt-stop-lines.md).

Contributions should preserve these rules:

1. Capability-first delivery: land real behavior, stronger verification, or an honest documentation correction.
2. TDD is required when behavior changes.
3. No fake green: do not weaken tests, hide regressions, or advertise unsupported capability.
4. Write-scope discipline: keep the diff inside the intended lane unless correctness forces a wider change.
5. Facade honesty: aria2-style responses may default or omit unstable fields, but they must not distort internal truth.
6. Do not relabel already-landed baseline work as future roadmap just because the docs are stale.

## Hard Governance Gates

Every meaningful change should respect the three active hard gates.

### 1. Stop-line grading

If dependency limits or architecture boundaries block parity, record the gap honestly instead of papering it over.

Use these grades:

- `core-blocking`
- `advanced-but-acceptable`
- `migration-only`

The current BitTorrent stop-line ledger lives in `crates/raria-bt/tests/bt_gap_ledger.rs`.

### 2. Dependency viability audit

Before leaning on dependency behavior, confirm the dependency can actually support the intended capability. The current high-value dependency set is:

- `librqbit`
- `reqwest`
- `suppaftp`
- `russh` / `russh-sftp`
- `redb`
- `jsonrpsee`

### 3. Write-scope and crate-boundary discipline

Each plan step should declare:

- a primary write area
- optional supporting write areas
- explicit forbidden areas

If you need to cross those boundaries, document why the wider change is unavoidable.

## Current Closeout Areas

The active repo-facing closeout work is:

1. baseline and docs alignment
2. BitTorrent stop-line and docs or RPC sync
3. Metalink daemon-path runtime evidence
4. verification-contract and closure-evidence maintenance

Old Step 1 through Step 4 implementation work is baseline now. Do not reopen it as if it were still the roadmap.

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
| `raria-rpc` | aria2-style JSON-RPC server and facade |
| `raria-cli` | CLI and daemon integration |

## Pull Request Expectations

1. Say which closeout lane, stop-line, or documentation correction the change addresses.
2. Add or tighten tests first when behavior changes.
3. Keep the diff inside the declared write scope whenever possible.
4. Run the verification commands that match the scope and report the actual result.
5. Update docs whenever capability claims, stop-lines, or operational guidance change.

## Verification Expectations

Use [`docs/verification-contract.md`](docs/verification-contract.md) as the durable repository standard.

- Do not claim tests pass without fresh command output.
- Do not claim late-stage closure from documentation alone.
- If a path is only covered at unit or RPC level, say so.
- If a gap is blocked upstream or by design, keep it explicit in the stop-line ledger.

## License

By contributing, you agree that your contributions will be licensed under Apache 2.0.
