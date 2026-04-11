# raria Practical Maturity Guide

This document is the English repo-facing companion to `.omx/plans/2026-04-11-raria-practical-maturity-plan.md`, which remains the detailed source of truth.

## Mission

Keep the project goal unchanged while changing the execution style:

- raria should become a mature Rust download engine.
- Download-core and BitTorrent are the top priorities.
- XML-RPC is permanently out of scope.
- Delivery should be progressive and capability-first, not a large upfront foundation rewrite.

## What Changed

The project is no longer following a heavy-governance, foundation-first plan. The active model is:

- light governance
- progressive capability maturity
- explicit write scope
- real feature output at each step
- local refactors only when the current structure blocks honest delivery

## Hard Governance

Only three hard gates remain active.

### Stop-line grading

If parity is blocked by a dependency or design boundary, record it honestly instead of pretending it works.

Required fields for a stop-line ledger entry:

- `gap_id`
- `feature`
- `grade`
- `blocking_dependency`
- `why_not_fixable_locally`
- `temporary_behavior`
- `evidence`

Allowed grades:

- `core-blocking`
- `advanced-but-acceptable`
- `migration-only`

### Dependency viability audit

Audit only the high-value dependencies that can meaningfully block the roadmap:

- `librqbit`
- `reqwest`
- `suppaftp`
- `russh` / `russh-sftp`
- `redb`
- `jsonrpsee`

For each one, capture:

- purpose
- current touchpoints
- known capability limits
- whether it creates a stop-line risk

### Write-scope discipline

Every roadmap step must declare:

- primary write crate
- supporting write crates
- forbidden crates

Do not turn a step into a cross-workspace refactor just because adjacent cleanup looks tempting.

## Immediate Truths the Docs Must Preserve

1. The current repository already has meaningful working code and tests across core, daemon, RPC, FTP/FTPS, SFTP, Metalink, and BT service integration.
2. The main shortfall is capability closure and honest projection, not the lack of a new grand abstraction.
3. The BT path is real but incomplete. Remaining gaps must be documented honestly instead of relabeled as finished.
4. The aria2-style facade exists as a migration/control layer and must not dictate internal truth.

## Shared Semantic Contract

The minimum shared contract that current work must preserve:

### Seeding

- `Seeding` should be a distinct internal state.
- The aria2-style facade may still project it as `"active"`.
- `tellActive` must include seeding tasks.
- Global stats should count seeding tasks as active and include upload speed.
- Restart behavior may conservatively restore a prior seeding task as `Waiting` unless the runtime can rebuild seeding honestly.

### BT download completion

- Internal event: `BtDownloadComplete`
- Facade event: `aria2.onBtDownloadComplete`
- It must fire once when payload download completes and seeding begins.
- Repeated polling of a complete torrent is not sufficient; a one-shot guard is required.

## Current Execution Order

### Step 1 — Minimal core semantic extension

Primary write: `raria-core`

Support write: `raria-rpc` (mechanical only), tests

Goal:

- add the minimum core semantics needed for BT seeding
- keep the existing `Job + Status + DownloadEvent` model
- avoid a large new object model or state-machine rewrite

### Step 2 — BT truth synchronization

Primary write: `raria-bt`

Support write: `raria-cli`

Goal:

- sync real BT fields back into internal state
- model `Active -> Seeding -> Complete`
- emit `BtDownloadComplete` exactly once

### Step 3 — Daemon capability closure

Primary write: `raria-cli`

Support write: `raria-core`

Goal:

- connect checksum verification to the real daemon lifecycle
- wire conditional GET into the daemon path
- add richer input-file parsing without breaking the old API
- introduce a first-pass transient/permanent error split without deep hierarchy surgery

### Step 4 — RPC / facade closure

Primary write: `raria-rpc`

Goal:

- project real BT fields honestly
- keep seeding visible through aria2-style active views
- default unsupported facade-only fields instead of mutating core truth to satisfy them

### Step 5 — Metalink enhancement

Primary write: `raria-metalink`

Support write: `raria-core`, `raria-range`

Goal:

- carry piece hashes, mirrors, and priority deeper into execution
- avoid turning Metalink work into a platform-wide source-graph redesign

### Step 6 — Verification closure

Primary write: tests + docs

Goal:

- run full workspace verification
- add key end-to-end coverage
- make docs describe only real capabilities

## Refactor Trigger Rules

A local refactor is justified only when at least one of these is true:

1. the current structure prevents the target capability from landing
2. the current structure prevents tests from expressing the target behavior
3. the step would otherwise require repeated patchwork in multiple places
4. stop-line information cannot be represented honestly without the change
5. BT field sprawl materially harms `Job` readability and a minimal `BtSnapshot`-style grouping becomes necessary

## Documentation Rules

When updating repo docs:

- write in English
- describe only verified or explicitly stop-lined behavior
- do not label existing integrated code as merely "planned" if tests and shipped code already exist
- do not claim parity where the plan explicitly calls out an unresolved gap
- keep README, CONTRIBUTING, and roadmap language aligned with this document
