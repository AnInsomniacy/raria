# raria Logging Contract

This document defines the current logging contract for `raria`.

It is intentionally narrower than a full observability platform design. The purpose is to give implementation lanes a stable, bounded contract for structured logging rollout.

## Scope

The current rollout tranche covers:

1. `raria-cli` daemon lifecycle and download loop
2. `raria-core` task lifecycle / progress / failure paths
3. `raria-rpc` control-surface mutations and notification emission

Additional emitters should not be added opportunistically. Widening the tranche requires an explicit follow-up plan step.

## Format

- Log output should be structured JSON when file logging is enabled.
- The schema should be stable enough for downstream tooling and incident analysis.
- Structured logs are preferred over freeform prose for machine parsing.

## Core Fields

At minimum, structured output should preserve:

- timestamp
- level
- target
- message
- event-specific fields emitted by the callsite

## Event Taxonomy

High-value event families in the current tranche:

- task lifecycle
- mirror selection / failover
- checksum and integrity failures
- BT lifecycle milestones
- restore / resume
- RPC control actions
- WS notification emission

## Correlation

Where the code has the information available, logs should include correlation-friendly identifiers such as:

- `gid`
- process or session context
- request or control context when relevant

The contract does not require every event to carry every identifier. It requires enough identifiers to reconstruct a single job lifecycle without source-diving.

Current bounded rollout behavior:

- daemon-backed structured logs carry `session_id` after the engine is initialized
- RPC control events emitted inside the daemon process inherit the same `session_id`

## Redaction Policy

The logging contract must not expose sensitive fields directly.

### Forbidden or redacted data

- RPC secrets
- HTTP passwords
- embedded URL credentials
- sensitive query parameters or bearer-like tokens

### Current enforcement

- credential-bearing URLs are redacted before logging in the highest-value daemon and single-download callsites
- repository prose must not claim broader redaction coverage than what is actually implemented

## Sink Ownership

Current sink behavior:

- stdout/stderr style console logging for interactive runs
- shared structured JSON file output when `--log <path>` is configured

The sink is installed from `raria-cli`, but the structured file emitter is shared from `raria-core` so `cli`, `core`, and `rpc` can emit into the same bounded contract.

The shared emitter also supports a process-wide structured context so daemon/session correlation can be injected once instead of re-adding the same field at every callsite.

## Verification

The current contract is considered satisfied only when all of these hold:

1. focused logging tests pass
2. daemon log-file output is valid JSON
3. the log file does not leak obvious secret material in the covered path
4. the broader crate/workspace verification still passes
5. high-value daemon and RPC control events appear in the structured file sink
6. BT lifecycle milestone events appear in the structured file sink for the daemon path
7. source-failed lifecycle events appear in the structured file sink when mirror failover occurs
8. terminal integrity failures appear in the structured file sink with the verification error

Representative evidence:

- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/tests/rpc_smoke.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/tests/rpc_smoke.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/main.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/main.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/util.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/util.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/single.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/single.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/logging.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/logging.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/engine.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/engine.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/bt_runtime.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/bt_runtime.rs)

## Non-goals

This contract does not yet promise:

- distributed tracing
- full OTLP export
- retention/rotation policy across deployment environments
- schema stability as a public external API
- complete redaction coverage for every possible field in every crate

Those can be expanded later, but should not be implied before implementation exists.
