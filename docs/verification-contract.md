# raria Verification Contract

This document is the durable repository contract for verification and closure evidence.

It does not claim that the repository is green right now. It defines:

- which commands must be rerun before a completion claim
- which automated tests back which user-facing paths
- which evidence is durable repository knowledge versus fresh-run evidence
- which gaps still block a late-stage closure declaration

## Evidence Types

Use two evidence categories and do not mix them.

### Durable repository evidence

These are facts that can be anchored to versioned code or tests:

- a specific automated test exists
- a stop-line gap is explicitly ledgerized
- a code path projects a specific field or semantic
- a docs file defines the current repository standard

Durable evidence supports statements such as "this path has automated coverage" or "this limitation is explicitly stop-lined."

### Fresh-run evidence

These are statements that require rerunning commands in the current state of the tree:

- `cargo test --workspace` passes
- `cargo check --workspace` passes
- `cargo clippy --workspace --all-targets -- -D warnings` passes
- a closeout step is complete in the current checkout
- the repository is ready for a closure declaration

Do not make fresh-run claims from static docs alone.

## Required Verification Matrix

The full closeout matrix is:

```bash
cargo test --workspace
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

These commands are the minimum bar for any claim that the current tree is verified end to end.

For narrower changes, focused checks are acceptable during iteration, but they do not replace the full matrix for a closure claim.

## Critical Path to Evidence Mapping

| User-facing path | Current evidence type | Primary anchors |
| --- | --- | --- |
| `addUri` daemon flow, terminal completion, and checksum failure handling | daemon-path automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| daemon shutdown, saved session restore, and resumed range requests after restart | daemon-path automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| conditional GET skips overwrite on `304 Not Modified` | daemon-path automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| `aria2.addMetalink` parsing, normalization, checksum wiring, and lightweight relation fields | RPC and engine automated coverage | `crates/raria-rpc/tests/metalink_dispatch.rs` |
| Metalink piece-checksum mismatch surfacing through daemon status and cleaning up the invalid output | daemon-path automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| Metalink mirror priority consumption and mirror failover on the daemon path | daemon-path automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| BT `Seeding` semantics, one-shot BT completion guard, and facade projection to active views | unit and RPC automated coverage | `crates/raria-core/src/job.rs`, `crates/raria-cli/src/bt_runtime.rs`, `crates/raria-rpc/src/facade.rs`, `crates/raria-rpc/tests/ws_parity.rs` |
| BT tracker option propagation and live peer details on the real daemon path | daemon-path automated coverage | `crates/raria-cli/tests/bt_tracker_smoke.rs` |
| BT `addTorrent`, `select-file`, `seed-ratio`, `seed-time`, and tracker option round-trips | RPC and engine automated coverage | `crates/raria-rpc/tests/bt_dispatch.rs`, `crates/raria-rpc/tests/options_parity.rs` |
| Structured daemon log file output, URL credential redaction, and RPC control-event capture | daemon-path automated coverage plus focused unit coverage | `crates/raria-cli/tests/rpc_smoke.rs`, `crates/raria-cli/src/main.rs`, `crates/raria-cli/src/util.rs`, `crates/raria-core/src/logging.rs`, `crates/raria-rpc/src/methods.rs` |
| Mirror failover emits `SourceFailed` events, WS notifications, and structured source-failure logs before successful completion | focused daemon-path, WS, and engine automated coverage | `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/tests/rpc_smoke.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-rpc/src/events.rs`, `crates/raria-rpc/src/server.rs`, `crates/raria-rpc/tests/ws_push.rs`, `crates/raria-rpc/tests/ws_parity.rs`, `crates/raria-rpc/tests/multicall_parity.rs` |
| Terminal checksum or piece-integrity rejection emits structured daemon verification-failure logs | daemon-path automated coverage | `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/tests/rpc_smoke.rs`, `crates/raria-cli/tests/session_smoke.rs` |
| Signal-driven daemon shutdown cancels throttled active downloads promptly instead of waiting for limiter sleep windows to drain | daemon-path automated coverage | `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-core/src/cancel.rs`, `crates/raria-core/src/limiter.rs`, `crates/raria-range/src/executor.rs` |
| Signal-driven single-download shutdown routes through engine-level cancellation and exits cleanly while throttled | single-path automated coverage | `crates/raria-cli/src/single.rs`, `crates/raria-cli/tests/single_download.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-core/src/cancel.rs`, `crates/raria-core/src/limiter.rs`, `crates/raria-range/src/executor.rs` |
| Known BT parity limits | explicit stop-line ledger | `crates/raria-bt/tests/bt_gap_ledger.rs` |

## Current Closure Gaps

The following items should remain explicit until fresh evidence closes them:

1. BitTorrent has strong unit, RPC, and daemon-path coverage for tracker announce and peer exposure, but the repo should not overstate that as full aria2 parity. The stop-line ledger remains authoritative for the unresolved parity gaps.
2. Outside the explicit BT stop-line ledger, repository-facing closure claims should track only verified, currently implemented behavior.

## Documentation Contract

Repository prose may say:

- a path has automated coverage
- a limitation is stop-lined
- a behavior is projected by current code
- a closeout area still needs fresh evidence

Repository prose should not say:

- the workspace currently passes unless the commands were rerun
- Metalink runtime closure is complete without daemon-path evidence
- BitTorrent parity is complete while the stop-line ledger still lists gaps
- documentation updates alone complete the closeout plan

## Minimum Review Checklist

Before claiming completion for a closeout step, verify:

1. the relevant docs match the current code and test anchors
2. any unsupported behavior is either defaulted honestly or stop-lined
3. the full verification matrix has been rerun if the claim is broader than a docs-only correction
4. the claim distinguishes durable repository evidence from fresh-run evidence
