# raria Modernization Runbook

This file is the authoritative recovery and execution document for completing raria as a modern Rust download manager. aria2 is a source reference for downloader capabilities only. raria does not preserve aria2 public API, CLI, configuration, session, control-file, storage, field-name, or ecosystem compatibility.

## Current State

The current branch contains the committed modernization snapshot `b18d3b7`. The tree compiles with `cargo check --workspace --locked`, `cargo fmt --all --check` passes, and `git diff --check` reports no whitespace errors. Recent work touched the native API, daemon runtime, BitTorrent runtime, native task model, native configuration, Metalink parsing and dispatch, FTP backend, native API tests, daemon smoke tests, and modernization docs.

The project is no longer a skeleton. It has working HTTP/HTTPS, FTP/FTPS, SFTP, Metalink, BitTorrent, segmented downloads, retry, resume, native API routes, native WebSocket events, redb-backed persistence, structured logs, and many daemon smoke tests. The work is not complete because major internals and tests still depend on aria2-shaped JSON-RPC, `Gid`, `Job`, compatibility terminology, and migration adapters.

The most recent completed checkpoint is Checkpoint 97, Native Session Task Creation and Status. The next checkpoint is Checkpoint 98, Native RPC Smoke Replacement. Its purpose is to migrate useful behavior from `rpc_smoke.rs` into native API smoke tests and delete pure JSON-RPC compatibility coverage when native coverage exists.

Current legacy-surface evidence includes remaining references to JSON-RPC methods, `Gid`, `gid`, `task_migration_`, `parity`, `compatibility`, and `legacy` outside the migrated session smoke tests. This is expected during the transition, but it is not acceptable at completion.

## Recovery Rules

Read this file first after every resume or context compaction. Do not treat prior chat summaries, old progress logs, or old matrix text as authoritative when they conflict with current source evidence.

Work only on the current branch. Do not create or switch branches. Do not read dependency source code or generated build output. Read raria source, tests, docs, and the current git diff. Read aria2 source, manual, and tests only to identify useful modern downloader behavior and legacy behavior that should be cut.

Before each checkpoint, search for existing raria implementation. Decide whether the checkpoint should reuse it, replace it, or delete it. Avoid adding another compatibility adapter when an existing native path should be finished. Prefer deleting legacy compatibility code after equivalent native tests exist.

At every checkpoint, update this runbook before code changes with the active target and update it again after validation. Keep code, tests, docs, comments, API names, configuration names, commit messages, and runbook entries in professional English.

Use internet search only when current external library behavior, protocol details, standards, latest versions, or public APIs cannot be proven from local project code and local aria2 reference material. Prefer local evidence first.

## Source Audit Scope

raria workspace: `/Users/sekiro/Projects/personal/raria`

aria2 reference tree: `/Users/sekiro/Projects/oss/aria2`

Included raria inputs are workspace manifests, crate manifests, all workspace Rust source, all workspace tests, repository Markdown docs, toolchain configuration, and the current git diff. Excluded raria inputs are `.git`, `target`, dependency source, generated build output, and unrelated editor artifacts.

Included aria2 inputs are `src`, `test`, `doc/manual-src/en/aria2c.rst`, `doc/manual-src/en/technical-notes.rst`, and other manual pages only when they identify downloader capability or legacy exclusions. Excluded aria2 inputs are dependency trees, generated build output, packaging-only platform baggage, and compatibility surfaces explicitly out of scope.

The aria2 manual and source identify modern downloader areas such as multi-source HTTP/FTP/SFTP downloads, segmented scheduling, checksums, Metalink v3/v4, BitTorrent metadata, DHT, UDP trackers, PEX, WebSeed, file selection, queueing, rate limits, proxy, TLS, cookies, netrc, persistence, hooks, process lifecycle, and structured control. aria2 source anchors include request groups, segment management, piece storage, URI selection, server stats, HTTP/FTP/SFTP commands, Metalink parsers, BitTorrent/DHT/tracker/PEX/metadata code, persistence, option handling, and RPC/event code. RPC/event code is used only to identify behavior categories, not to preserve the aria2 interface.

## Modern Architecture Contract

raria public surfaces are native. The final product uses `raria.toml`, `/api/v1` HTTP JSON resources, `/api/v1/events` WebSocket events, versioned raria persistence schemas, native CLI commands and field names, and opaque task identifiers. Public names must describe raria concepts, not aria2 compatibility.

The native domain model is centered on `TaskId`, `Task`, `Source`, `FileEntry`, `Segment`, `Piece`, `Peer`, `Tracker`, and typed events. `Gid`, `Job`, `task_migration_`, and aria2-shaped status names are migration debt. They may remain temporarily inside private adapters while checkpoints replace them, but they are not completion-compatible.

The native lifecycle is `queued`, `running`, `paused`, `seeding`, `completed`, `failed`, and `removed`. The control API must expose these states and native resource names without `gid`, `jsonrpc`, `aria2.*`, token-in-params auth, or aria2 option keys.

The native configuration model is strict TOML with denied unknown fields. Legacy key-value configuration parsing, aria2 option names, and compatibility aliases must be removed from public and test surfaces. Runtime internals may retain transitional fields only until native task/config ownership replaces them.

Persistence must use versioned raria schemas. redb is the storage engine, not the schema model. Direct serialization of migration `Job` rows, old `Gid` segment rows, and runtime bridge IDs must be removed after native task, source, file, segment, piece, tracker, and external BitTorrent state references are covered by tests.

Protocol implementation should use mature Rust libraries already chosen by the project where possible: reqwest for HTTP/HTTPS, suppaftp for FTP/FTPS, russh for SFTP, quick-xml for Metalink, librqbit for BitTorrent, redb for local persistence, and axum for the native API and WebSocket stream. Missing modern behavior should be implemented as the smallest robust raria-native layer with focused tests.

## Modern Feature Matrix

| Area | Capability | Modern target | Local evidence | Status | Closing checkpoint |
| --- | --- | --- | --- | --- | --- |
| Public surface | Native HTTP API | `/api/v1` resource API for health, config, stats, task creation, task reads, task controls, transfer policy, queue, files, sources, peers, trackers, seeding, session save, and daemon shutdown | `crates/raria-rpc/src/api.rs`, `crates/raria-rpc/tests/native_api.rs`, `crates/raria-cli/tests/native_api_smoke.rs` | Partial | CP97-CP103 |
| Public surface | Native event stream | `/api/v1/events` emits typed raria event names and payloads without aria2 notification envelopes | `NativeEvent`, native event bus, daemon native event smoke tests; legacy event fallback still exists | Partial | CP100 |
| Public surface | Native CLI | CLI commands and options use raria names and native config/API concepts | `crates/raria-cli/src/main.rs`, native `--api-port`; many aria2-shaped options remain | Gap | CP106 |
| Public surface | User documentation | Docs describe raria-native behavior only, with no compatibility claims except historical migration notes | README partially updated; old modernization docs and some crate docs still use compatibility wording | Partial | CP107 |
| Configuration | Strict `raria.toml` | Native sections for daemon, API, downloads, network, HTTP, FTP, SFTP, BitTorrent, Metalink, storage, logging, hooks, and security | `crates/raria-core/src/native_config.rs`; old parser remains | Partial | CP104 |
| Persistence | Versioned native store | Versioned task, source, file, segment, piece, tracker, event cursor, config, migration ledger, and external BT state references | Native metadata/task rows and native segments exist; `Job` rows and `Gid` fallback remain | Partial | CP109-CP113 |
| Identity | Opaque task IDs | Public and internal task ownership use opaque `TaskId`, not aria2 GID semantics | `TaskId` exists; `Gid`, `task_migration_`, runtime bridge IDs remain | Partial | CP108-CP112 |
| Core runtime | Native task model | Protocol-neutral task graph with files, sources, segments, pieces, peers, trackers, policy, timestamps, and errors | Native projections exist; `Job` drives runtime state | Partial | CP108 |
| Core runtime | Queue scheduling | Native queued/running/paused/seeding/completed/failed/removed scheduling with bounded active tasks and priorities | Scheduler now stores native task IDs; legacy queue adapters remain | Partial | CP109 |
| Core runtime | Lifecycle controls | Pause, resume, remove, restart, shutdown, and session save operate through native task service | Native API has controls; engine still bridges to GID operations | Partial | CP97-CP109 |
| Core runtime | Progress and stats | Accurate per-task and global completed bytes, total bytes, speed, connections, ETA, and lifecycle counts | Native stats route exists; old event bus still feeds some paths | Partial | CP100 |
| Core runtime | Runtime mutation | Safe mutation of limits, queue position, sources, file selection, trackers, and seeding policy | Native routes exist for several mutations; BT source graph and priorities incomplete | Partial | CP101-CP103 |
| Core runtime | Structured logs | JSONL operational logs with redaction and task correlation | `docs/logging-contract.md`, logging helpers, daemon smoke tests | Partial | CP107 |
| Core runtime | Hooks | Modern lifecycle hooks or event-consumer model | start/complete/error hooks exist but still expose legacy identifiers in tests | Partial | CP105 |
| HTTP/HTTPS | Basic transfers | Redirects, headers, auth, TLS, mTLS, cookies, netrc, proxy, compression, range, conditional GET, resume, remote metadata | `raria-http`, `single_download`, `http_config_smoke`, session smoke | Partial | CP114 |
| FTP/FTPS | FTP transfers | FTP and explicit FTPS with auth, passive mode where useful, resume, proxy, remote metadata | `raria-ftp`, FTP/FTPS smoke tests | Partial | CP115 |
| SFTP | SFTP transfers | Password/key auth, known_hosts, proxy, resume, metadata | `raria-sftp`, SFTP smoke tests | Partial | CP116 |
| Multi-source | Mirror failover | Multi-source tasks choose healthy mirrors, record failures, retry, and expose source health | source health and failover tests exist | Partial | CP117 |
| Multi-source | Adaptive segments | Segment planning reacts to source health and runtime progress, including live rebalance | planning reacts to health; live rebalance is missing | Gap | CP118 |
| Resume | Crash recovery | Restart restores resumable range and BT tasks from native schemas | native segment rows and session restore exist; bridge rows remain | Partial | CP112 |
| Integrity | Whole-file checksum | Enforce modern whole-file checksums and fail safely | checksum module and Metalink checksum daemon tests | Covered | Regression |
| Integrity | Piece checksum | Unified piece model for Metalink and BitTorrent verification | Metalink piece checks and BT metadata exist; no unified piece ownership | Partial | CP119 |
| Disk | File allocation | none, prealloc, trunc, falloc where supported, with resume-safe behavior | `file_alloc.rs`, native config enum | Partial | CP120 |
| Disk | Conflict policy | rename, overwrite, reuse partial with validators, fail on conflict | rename/overwrite and BT selected cleanup exist | Partial | CP121 |
| Metalink | v3/v4 parsing | Parse useful modern Metalink inputs and normalize into native tasks | parser and normalizer exist; v4 metaurl handled | Partial | CP122 |
| Metalink | Mirror filters | location, protocol preference, priority, and unique-protocol behavior | normalizer and raria.toml preferences exist | Partial | CP122 |
| Metalink | Multi-file graph | Multi-file Metalink creates native task graph or task collection with shared metadata | native creation exists, lightweight relation model remains | Partial | CP123 |
| Metalink | Torrent metaurl | Torrent metadata sources from Metalink create BT tasks with WebSeed mirrors | native dispatch exists | Partial | CP123 |
| BitTorrent | Torrent files | Local, bytes, and remote torrent metadata create native BT tasks | native smoke tests exist | Partial | CP124 |
| BitTorrent | Magnet metadata | Magnet tasks resolve metadata and publish native events | librqbit path exists; daemon-level metadata lifecycle incomplete | Partial | CP125 |
| BitTorrent | DHT | DHT enablement, bootstrap, persistence, shutdown save, and native config | DHT persistence tests exist | Partial | CP126 |
| BitTorrent | UDP trackers | UDP tracker announce and native tracker projection | local UDP tracker runtime and daemon smoke tests exist | Covered | Regression |
| BitTorrent | PEX | Peer exchange when supported, disabled for private torrents or native policy where enforceable | policy field and handshake evidence exist; enforcement incomplete | Partial | CP127 |
| BitTorrent | WebSeed | BEP-17/BEP-19 WebSeed download, status, failure, and selected-file behavior | WebSeed runtime and daemon tests exist | Partial | CP128 |
| BitTorrent | File selection | Pre-download and live selection, persistence, progress, cleanup of unselected files | native routes and daemon tests exist; legacy option remains | Partial | CP129 |
| BitTorrent | Tracker management | Add, exclude, timeout, interval, replacement, and status projection | native tracker routes and snapshots exist; backend enforcement incomplete | Partial | CP130 |
| BitTorrent | Peer projection | Native peer list with address, speeds, direction, seeder, client, and flags where available | basic peer projection exists | Partial | CP131 |
| BitTorrent | Seeding controls | ratio, time, idle stop, seed-only lifecycle, scheduler detachment, upload limits | partial runtime and daemon coverage exists | Partial | CP132 |
| BitTorrent | Fastresume | Durable restore of BT progress through native session state and external backend state references | fastresume directory binding exists | Partial | CP133 |
| Transfer policy | Rate limiting | Global and per-task download/upload limits across range and BT | range limiter exists; BT upload enforcement incomplete | Partial | CP134 |
| Transfer policy | Retry model | Typed transient/permanent error classification with retry budget and backoff | retry and daemon classification exist | Partial | CP135 |
| Network | DNS and interface | DNS controls, bind address/interface, IPv6 policy where modern libraries support them | aria2 has options; raria lacks full model | Gap | CP136 |
| Process | Daemon lifecycle | native daemon mode, signal handling, graceful shutdown, stop timer, stop-with-process | shutdown route and signal handling exist; timers incomplete | Partial | CP137 |
| Security | Auth and redaction | bearer auth, path safety, credential redaction, TLS/mTLS config, secret handling | native bearer auth exists; JSON-RPC secret remains | Partial | CP138 |
| Cleanup | Legacy public surfaces | Remove JSON-RPC, aria2 method names, aria2 option names, compatibility docs, and parity tests after native coverage | JSON-RPC surface still registered except addTorrent/addMetalink | Gap | CP139-CP142 |

## Migration Debt Register

| Debt | Evidence | Risk | Removal condition | Closing checkpoint |
| --- | --- | --- | --- | --- |
| JSON-RPC server | `crates/raria-rpc/src/server.rs`, `/jsonrpc`, `aria2.*` methods, RPC smoke tests | Keeps aria2 API alive and distorts design | Native API and event tests cover all useful behavior | CP139 |
| aria2 method names | `aria2.addUri`, `aria2.tellStatus`, `aria2.shutdown`, `aria2.saveSession`, old RPC tests | Encourages compatibility instead of native workflow | Session, daemon, logging, and protocol smoke tests use `/api/v1` only | CP97-CP103 |
| `Gid` and `task_migration_` | `crates/raria-core/src/job.rs`, `engine.rs`, `scheduler.rs`, native index bridge | Prevents native task ownership and schema cleanup | Engine, scheduler, cancellation, events, logs, and persistence own `TaskId` | CP108-CP112 |
| `Job` runtime model | `Job` drives persisted runtime state and projections | Native task graph remains a facade | Native `Task` rows and in-memory state replace `Job` | CP108 |
| legacy config parser | `crates/raria-core/src/config_file.rs`, `GlobalConfig` aria2 names | Keeps old config semantics alive | CLI and tests use strict `raria.toml`; parser removed or private test fixture only | CP104 |
| parity tests | `rpc_parity.rs`, `ws_parity.rs`, `multicall_parity.rs`, `options_parity.rs`, `ws_push.rs` | Tests the wrong product contract | Useful behavior moved to native contract tests, pure compatibility tests deleted | CP139-CP142 |
| legacy event projection | `crates/raria-rpc/src/events.rs`, fallback from `DownloadEvent` to native | Event model remains partly aria2-shaped | Native event bus is sole daemon stream | CP100 |
| legacy persistence fallback | old `Gid` segment rows, `NativeTaskRow::from_job_for_migration` | Blocks schema finalization | Migration fixtures prove cutover; old rows removed | CP112 |
| compatibility wording | crate docs, README, old modernization docs, comments | Misleads future agents | Docs describe native raria only; old docs archived or rewritten | CP107 |
| BT legacy encryption config | `BtMinCryptoLevel`, `bt_require_crypto`, aria2 crypto comments | Goal excludes MSE/ARC4 | Removed or reclassified as unsupported modern transport policy | CP138 |

## Excluded Legacy And Edge Features

These are not implementation targets. Remove public options, tests, docs, and compatibility code for them when encountered.

| Feature | Decision |
| --- | --- |
| XML-RPC | Delete. It is a legacy control surface. |
| libaria2 C API compatibility | Delete. raria is not an embedding-compatible library replacement. |
| aria2 session/control-file compatibility | Delete. raria uses versioned native schemas. |
| aria2 config syntax and option names | Delete. raria uses strict `raria.toml` and native CLI names. |
| HTTP pipelining | Delete. It is old HTTP/1.1 behavior and not a modern target. |
| BitTorrent MSE/ARC4 | Delete. It is old crypto behavior and explicitly out of scope. |
| LPD | Delete. It is explicitly out of scope. |
| AriaNg/Motrix compatibility | Delete. raria does not preserve the aria2 ecosystem. |
| JSON-RPC batch/multicall compatibility | Delete after native client workflows cover useful batch operations. |
| RPC token-in-params auth | Delete. Use bearer auth or another native HTTP auth model. |
| deprecated RPC user/password | Delete. aria2 itself marks it deprecated. |
| historical packaging/platform compatibility | Delete unless it affects a modern supported platform. |
| old control-file/session migration fixtures | Delete unless needed to migrate raria's own previous native schema. |
| UI color output and terminal cosmetics | Low priority. Keep only if it serves modern CLI UX. |
| disk cache/mmap | Evaluate after native core migration. Implement only with clear modern value and tests. |

## Checkpoint Ledger

### Checkpoint 96: Native Session Smoke Shutdown and Save

Status: complete

Scope: migrated session smoke shutdown and explicit session save from JSON-RPC to native endpoints.

Files: `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-rpc/src/api.rs`, `crates/raria-rpc/tests/native_api.rs`, `crates/raria-cli/tests/native_api_smoke.rs`, `crates/raria-cli/tests/bt_tracker_smoke.rs`

Validation: focused session, native API, and BT tracker tests passed in the prior run; current `cargo check --workspace --locked` also passes.

Evidence: session smoke no longer uses `aria2.shutdown` or `aria2.saveSession` for daemon shutdown/session save, but still uses `aria2.addUri` and `aria2.tellStatus`.

Remaining: migrate task creation and status polling to native API.

Next: Checkpoint 97.

### Checkpoint 97: Native Session Task Creation and Status

Status: complete

Scope: migrated `crates/raria-cli/tests/session_smoke.rs` from JSON-RPC task creation and status polling to native `/api/v1/tasks`.

Files: `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-cli/src/hooks.rs`, `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/src/single.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-core/src/native.rs`, `crates/raria-rpc/src/api.rs`, `crates/raria-rpc/src/methods.rs`

Validation: `cargo test -p raria-cli --test session_smoke` passed with 18 tests. `cargo check --workspace --locked` passed.

Evidence: `session_smoke.rs` now creates tasks through `POST /api/v1/tasks`, polls task state through `GET /api/v1/tasks/{taskId}` and `GET /api/v1/tasks`, uses `/api/v1/health` for readiness, and no longer calls `aria2.addUri`, `aria2.tellStatus`, `aria2.tellActive`, `aria2.tellWaiting`, `aria2.tellStopped`, or `aria2.getUris`. Hooks receive native task identifiers. Native task creation accepts checksum metadata, and failed native task summaries expose `errorMessage`.

Remaining: migrate `rpc_smoke.rs` behavior into native smoke tests or delete pure compatibility cases.

Next: Checkpoint 98.

### Checkpoint 98: Native RPC Smoke Replacement

Status: next

Scope: replace useful daemon/RPC smoke behavior with native API smoke coverage, then remove pure JSON-RPC compatibility assertions.

Files: `crates/raria-cli/tests/rpc_smoke.rs`, `crates/raria-cli/tests/native_api_smoke.rs`, `crates/raria-rpc/tests/native_api.rs`, native API and daemon code as needed.

Validation: start with focused native smoke tests, then run `cargo test -p raria-cli --test native_api_smoke`, `cargo test -p raria-rpc --test native_api`, and `cargo check --workspace --locked`.

Evidence target: useful daemon control, task lifecycle, authentication, stats, and task mutation behavior is covered through `/api/v1` resources without JSON-RPC method names, aria2 option names, public `gid`, or compatibility envelopes.

Remaining after completion: continue native event stream and public surface cleanup.

Next: Checkpoint 99.

## Validation Contract

Run the smallest relevant command at each checkpoint. Record the command and result in the checkpoint ledger.

Use these commands as the main validation ladder:

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test -p raria-cli --test session_smoke
cargo test -p raria-cli --test native_api_smoke
cargo test -p raria-rpc --test native_api
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Run `cargo check --workspace --locked` after meaningful integration slices. Run full workspace test and clippy only after a group of checkpoints is coherent enough to justify the time.

## Completion Definition

raria is complete when this runbook's feature matrix has no undocumented gaps; every non-implemented item is either an excluded legacy/edge feature or a proven technical limitation; all modern downloader features have tests or explicit verification anchors; raria-native API, configuration, CLI, events, persistence, and documentation have replaced aria2-style public surfaces; and `cargo test --workspace`, `cargo check --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` all pass.
