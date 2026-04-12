# raria Practical Maturity Guide

This document is the English repo-facing maturity companion for the current repository state.

It is not a future roadmap for work that already landed. The purpose of this guide is to keep repository prose aligned with current code and tests.

See also:

- [`bt-stop-lines.md`](bt-stop-lines.md) for explicit BitTorrent parity limits
- [`verification-contract.md`](verification-contract.md) for the durable verification standard
- [`logging-contract.md`](logging-contract.md) for the current logging rollout contract

## Mission

The project goal has not changed:

- raria should become a mature Rust download engine
- download-core and BitTorrent remain the highest priorities
- XML-RPC is permanently out of scope

What changed is the execution posture:

- old Step 1 through Step 4 implementation work is baseline now
- the remaining work is closeout work, not a foundation rebuild
- repository docs must describe current truth, not outdated sequencing

## Current Baseline

The following areas already exist in code and should not be described as future work:

- core BT semantics include a distinct `Seeding` state and a one-shot BT completion guard in `raria-core`
- daemon and RPC flows already cover session restore, conditional GET, checksum failure reporting, and aria2-style status or global-stat projection
- Metalink already parses, normalizes, stores checksum and relation fields, and projects those fields through RPC
- BitTorrent integration already exists across runtime, daemon, and facade layers, with explicit parity limits recorded in the stop-line ledger

Representative anchors:

- `crates/raria-core/src/job.rs`
- `crates/raria-cli/src/bt_runtime.rs`
- `crates/raria-cli/src/daemon.rs`
- `crates/raria-rpc/src/facade.rs`
- `crates/raria-cli/tests/session_smoke.rs`
- `crates/raria-cli/tests/bt_tracker_smoke.rs`
- `crates/raria-rpc/tests/metalink_dispatch.rs`
- `crates/raria-bt/tests/bt_gap_ledger.rs`

## Active Closeout Scope

The remaining repo-facing work is deliberately narrower.

### 1. Baseline and docs alignment

Keep `README.md`, this guide, and contribution guidance aligned with the implemented baseline.

### 2. BitTorrent stop-line honesty

Keep BitTorrent docs, RPC behavior, and the stop-line ledger synchronized.

Known ledgerized gaps:

- `BT-GAP-001`: MSE or PSE encryption
- `BT-GAP-002`: WebSeed support
- `BT-GAP-003`: rarest-first piece selection
- `BT-GAP-004`: mixed range plus BitTorrent download of the same file

Those gaps are explicit limits, not hidden TODOs. The repository-facing ledger lives in [`bt-stop-lines.md`](bt-stop-lines.md), with the ignored parity-gap tests as the executable backing source.

### 3. Metalink daemon-path runtime closure

Metalink is no longer parser-only work. The repository already proves:

- parser and normalizer coverage
- job creation through `aria2.addMetalink`
- checksum and piece-checksum wiring
- relation-field projection through the aria2-style facade

The repository now proves daemon-path consumption of sorted Metalink mirrors, failover after an upstream mirror error, and failover after checksum or piece-checksum rejection. That means the closeout task is no longer to invent new Metalink runtime semantics, but to keep docs aligned with the verified daemon-path behavior.

### 4. Verification closure

Completion claims require fresh evidence, not static prose. The versioned standard lives in [`verification-contract.md`](verification-contract.md).

## Hard Governance

Only three hard gates remain active.

### Stop-line grading

If parity is blocked by a dependency or design boundary, record it honestly instead of pretending it works.

Required stop-line fields:

- `gap_id`
- `feature`
- `grade`
- `blocking_dependency`
- `why_not_fixable_locally`
- `temporary_behavior`
- `evidence`

### Dependency viability audit

Focus the audit on dependencies that can materially block the roadmap:

- `librqbit`
- `reqwest`
- `suppaftp`
- `russh` / `russh-sftp`
- `redb`
- `jsonrpsee`

### Write-scope discipline

Each execution step should still declare:

- primary write area
- supporting write areas
- forbidden areas

Do not turn a closeout step into a cross-workspace refactor just because cleanup looks attractive nearby.

## Documentation Rules

When updating repo docs:

- write in English
- describe only implemented behavior, explicit stop-lines, or clearly labeled evidence gaps
- do not describe old Step 1 through Step 4 work as pending roadmap
- do not claim parity where the stop-line ledger says otherwise
- keep README, CONTRIBUTING, and verification guidance aligned

## Shared Truths the Docs Must Preserve

1. The repository already contains meaningful working code and automated coverage across core, daemon, RPC, HTTP, FTP, SFTP, Metalink, and BitTorrent integration surfaces.
2. The main remaining problem is closure evidence and honest projection, not the absence of a new architecture skeleton.
3. The BitTorrent path is real but bounded. Remaining gaps must stay explicit.
4. The aria2-style facade is a migration and control surface. It must project internal truth rather than redefine it.
5. Logging/diagnostics work must stay bounded: first a contract, then bounded rollout across the highest-value runtime surfaces.
