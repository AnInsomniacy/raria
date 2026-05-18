# raria Modernization Runbook

This file is the authoritative recovery and execution document for completing raria as a modern Rust download manager. aria2 is a source reference for downloader capabilities only. raria does not preserve aria2 public API, CLI, configuration, session, control-file, storage, field-name, or ecosystem compatibility.

## Current State

The current branch contains the committed modernization stream through Checkpoint 111. The tree compiled with `cargo check --workspace --locked`, `cargo fmt --all --check` passed, and `git diff --check` reported no whitespace errors at the end of Checkpoint 111. Recent work touched the native API, daemon runtime, BitTorrent runtime, native task model, native configuration, Metalink parsing and dispatch, FTP backend, native API tests, daemon smoke tests, and modernization docs.

The project is no longer a skeleton. It has working HTTP/HTTPS, FTP/FTPS, SFTP, Metalink, BitTorrent, segmented downloads, retry, resume, native API routes, native WebSocket events, redb-backed persistence, structured logs, and many daemon smoke tests. The work is not complete because major internals and tests still depend on aria2-shaped JSON-RPC, `Gid`, `Job`, compatibility terminology, and migration adapters.

The most recent completed checkpoint is Checkpoint 112, Native Identity And Persistence Cleanup. The next checkpoint is Checkpoint 113, Native Persistence Schema Boundary. Its purpose is to continue replacing direct `Job` row ownership with versioned native persistence rows.

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
| Public surface | Native CLI | CLI commands and options use raria names and native config/API concepts | `crates/raria-cli/src/main.rs`, native `--api-port` and native hook names; legacy RPC and BT crypto flags removed; retry help text no longer references aria2; many aria2-shaped options remain | Gap | CP111 |
| Public surface | User documentation | Docs describe raria-native behavior only, with no compatibility claims except historical migration notes | README partially updated; old modernization docs and some crate docs still use compatibility wording | Partial | CP107 |
| Configuration | Strict `raria.toml` | Native sections for daemon, API, downloads, network, HTTP, FTP, SFTP, BitTorrent, Metalink, storage, logging, hooks, and security | `crates/raria-core/src/native_config.rs`; hooks are native TOML; old aria2-style key-value parser deleted | Partial | CP111 |
| Persistence | Versioned native store | Versioned task, source, file, segment, piece, tracker, event cursor, config, migration ledger, and external BT state references | Native metadata/task rows and native segments exist; direct `Job` rows and explicit private runtime bridge IDs remain | Partial | CP109-CP113 |
| Identity | Opaque task IDs | Public and internal task ownership use opaque `TaskId`, not aria2 GID semantics | `TaskId` owns task queue, persisted task rows, and native API identity; deterministic `task_migration_` generation and fallback decoding were removed; `Gid` remains as a private runtime bridge | Partial | CP108-CP112 |
| Core runtime | Native task model | Protocol-neutral task graph with files, sources, segments, pieces, peers, trackers, policy, timestamps, and errors | Native projections exist; `Job` drives runtime state | Partial | CP108 |
| Core runtime | Queue scheduling | Native queued/running/paused/seeding/completed/failed/removed scheduling with bounded active tasks and priorities | Scheduler now stores native task IDs; legacy queue adapters remain | Partial | CP109 |
| Core runtime | Lifecycle controls | Pause, resume, remove, restart, shutdown, and session save operate through native task service | Native API has controls; engine still bridges to GID operations | Partial | CP97-CP109 |
| Core runtime | Progress and stats | Accurate per-task and global completed bytes, total bytes, speed, connections, ETA, and lifecycle counts | Native task projection now exposes output path, timestamps, segments, active connections, transfer limits, ETA, progress, and lifecycle; old event bus still feeds some paths | Partial | CP100 |
| Core runtime | Runtime mutation | Safe mutation of limits, queue position, sources, file selection, trackers, and seeding policy | Native routes exist for several mutations; BT source graph and priorities incomplete | Partial | CP101-CP103 |
| Core runtime | Structured logs | JSONL operational logs with redaction and task correlation | `docs/logging-contract.md`, logging helpers, daemon smoke tests | Partial | CP107 |
| Core runtime | Hooks | Modern lifecycle hooks or event-consumer model | task start, completion, and failure hooks use native CLI names, strict `[hooks]` TOML, native task identifiers, and native task summary projection for hook arguments | Covered | Regression |
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
| `Gid` runtime bridge | `crates/raria-core/src/job.rs`, `engine.rs`, native index bridge | Prevents final native task ownership and schema cleanup | Engine, cancellation, events, logs, and persistence own `TaskId` without `Job` bridge rows | CP113 |
| `Job` runtime model | `Job` drives persisted runtime state and projections | Native task graph remains a facade | Native `Task` rows and in-memory state replace `Job` | CP108 |
| legacy config parser | Historical `crates/raria-core/src/config_file.rs` module | Kept old config semantics alive | Deleted; runtime config uses strict native `raria.toml` | CP111 |
| parity tests | `rpc_parity.rs`, `ws_parity.rs`, `multicall_parity.rs`, `options_parity.rs`, `ws_push.rs` | Tests the wrong product contract | Useful behavior moved to native contract tests, pure compatibility tests deleted | CP139-CP142 |
| legacy event projection | `crates/raria-rpc/src/events.rs`, fallback from `DownloadEvent` to native | Event model remains partly aria2-shaped | Native event bus is sole daemon stream | CP100 |
| legacy persistence fallback | old `Gid` segment rows, `NativeTaskRow::from_job_for_migration` | Blocks schema finalization | Migration fixtures prove cutover; old rows removed | CP113 |
| compatibility wording | crate docs, README, old modernization docs, comments | Misleads future agents | Docs describe native raria only; old docs archived or rewritten | CP107 |
| BT legacy encryption config | aria2 `bt-require-crypto` and `bt-min-crypto-level` public surfaces | Goal excludes MSE/ARC4 | Removed from CLI, key-value config ownership, runtime forwarding, and BT compatibility tests | CP110 |

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

Evidence: session smoke no longer uses `aria2.shutdown` or `aria2.saveSession` for daemon shutdown/session save.

Remaining: task creation and status polling moved to the next checkpoint.

Next: Checkpoint 97.

### Checkpoint 97: Native Session Task Creation and Status

Status: complete

Scope: migrated `crates/raria-cli/tests/session_smoke.rs` from JSON-RPC task creation and status polling to native `/api/v1/tasks`.

Files: `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-cli/src/hooks.rs`, `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/src/single.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-core/src/native.rs`, `crates/raria-rpc/src/api.rs`, `crates/raria-rpc/src/methods.rs`

Validation: `cargo test -p raria-cli --test session_smoke` passed with 18 tests. `cargo check --workspace --locked` passed.

Evidence: `session_smoke.rs` now creates tasks through `POST /api/v1/tasks`, polls task state through `GET /api/v1/tasks/{taskId}` and `GET /api/v1/tasks`, uses `/api/v1/health` for readiness, and no longer calls `aria2.addUri`, `aria2.tellStatus`, `aria2.tellActive`, `aria2.tellWaiting`, `aria2.tellStopped`, or `aria2.getUris`. Hooks receive native task identifiers. Native task creation accepts checksum metadata, and failed native task summaries expose `errorMessage`.

Remaining: migrate broader RPC smoke behavior into native smoke tests or delete pure compatibility cases.

Next: Checkpoint 98.

### Checkpoint 98: Native RPC Smoke Replacement

Status: complete

Scope: replace useful daemon/RPC smoke behavior with native API smoke coverage, then remove pure JSON-RPC compatibility assertions.

Files: `crates/raria-cli/tests/native_api_smoke.rs`, `crates/raria-core/src/native.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-rpc/src/api.rs`, `crates/raria-rpc/src/methods.rs`, `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/src/single.rs`, `crates/raria-cli/src/bt_runtime.rs`, `crates/raria-cli/tests/rpc_smoke.rs`

Validation: focused native smoke tests for native task headers, auth, active connections, file-not-found retry budget, structured log redaction, and daemonize passed. `cargo check --workspace --locked` passed before the final file deletion and must be rerun with the full checkpoint validation ladder after this ledger update.

Evidence: useful behavior from `rpc_smoke.rs` now has native coverage for task creation, task status polling, request headers, HTTP Basic auth, active connection projection, retry budget, structured log file creation and credential redaction, native shutdown, daemon detach readiness, pause/resume, transfer policy mutation, source mutation, mirror failover, integrity failures, restore, native events, and BT lifecycle. Pure JSON-RPC CORS, JSON-RPC WebSocket origin, aria2 method, aria2 notification, AriaNg/Motrix-style compatibility, and RPC control-log assertions were deleted with `crates/raria-cli/tests/rpc_smoke.rs`.

Remaining after completion: finish native task status projection and remove remaining status-shape leakage in public surfaces.

Next: Checkpoint 99.

### Checkpoint 99: Native Task Status Projection

Status: complete

Scope: complete native task detail/list fields for status, progress, transfer, source health, files, and terminal errors without aria2-style names or migration identifiers.

Files: `crates/raria-core/src/native.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-rpc/src/api.rs`, `crates/raria-cli/tests/native_api_smoke.rs`, `crates/raria-rpc/tests/native_api.rs`

Validation: `cargo test -p raria-rpc --test native_api tasks_endpoint_returns_native_task_projection` passed. `cargo test -p raria-rpc --test native_api task_detail_pause_and_resume_use_native_task_id` passed. `cargo test -p raria-rpc --test native_api` passed with 33 tests. `cargo test -p raria-cli --test native_api_smoke` passed with 27 tests. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed.

Evidence: native task list and detail responses expose stable raria fields for `taskId`, `lifecycle`, `outputPath`, files, sources, `segments`, completed bytes, total bytes, download speed, active connections, estimated seconds remaining, transfer limits, creation time, update time, and terminal error. Native API tests assert that `gid`, aria2 `status`, `completedLength`, and `downloadSpeed` fields are absent from task projections.

Remaining after completion: native event stream cleanup.

Next: Checkpoint 100.

### Checkpoint 100: Native Event Stream Cleanup

Status: complete

Scope: make `/api/v1/events` use raria-native event envelopes only, remove legacy JSON-RPC event fallback from public behavior, and keep useful lifecycle/progress/source/BT event coverage.

Files: `crates/raria-rpc/src/api.rs`, `crates/raria-rpc/tests/native_api.rs`

Validation: `cargo test -p raria-rpc --test native_api native_events_websocket_ignores_legacy_download_event_bus` failed before implementation and passed after the fallback removal. `cargo test -p raria-rpc --test native_api native_events_websocket_streams_native_source_failures` passed. `cargo test -p raria-rpc --test native_api native_events_websocket_streams_native_lifecycle_events` passed. `cargo test -p raria-rpc --test native_api` passed with 33 tests. `cargo test -p raria-cli --test native_api_smoke` passed with 27 tests. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: `/api/v1/events` now subscribes only to the native event bus. The legacy `DownloadEvent` conversion fallback and its GID-to-task bridge helper were removed from the native API. Native WebSocket tests prove legacy `DownloadEvent` bus messages are ignored, while native lifecycle and source failure events still stream with typed raria event records and without `jsonrpc`, aria2 method names, or `gid` fields.

Remaining after completion: native task mutation cleanup.

Next: Checkpoint 101.

### Checkpoint 101: Native Task Mutation Cleanup

Status: complete

Scope: finish native mutation routes for task transfer policy, queue position, sources, files, trackers, and seeding policy; remove any remaining compatibility assertions from these public task mutation flows after equivalent native tests exist.

Files: `crates/raria-rpc/src/api.rs`, `crates/raria-rpc/tests/native_api.rs`, `crates/raria-cli/tests/native_api_smoke.rs`

Validation: `cargo test -p raria-rpc --test native_api task_mutation_routes_return_native_not_found_errors` failed before implementation and passed after missing-task detection was added. `cargo test -p raria-rpc --test native_api` passed with 34 tests. `cargo test -p raria-cli --test native_api_smoke` passed with 27 tests. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: native task mutation PATCH routes now check native task existence before mapping validation errors, so missing task resources return `task_not_found` with HTTP 404 instead of a generic invalid request. Mutation tests now use a shared recursive assertion to reject legacy public fields across tracker, seeding, transfer, source, queue, and file-selection responses, including `gid`, `jsonrpc`, aria2 option names, and compatibility field names.

Remaining after completion: native API route shape and documentation cleanup.

Next: Checkpoint 102.

### Checkpoint 102: Native API Route Shape and README Cleanup

Status: complete

Scope: align public route documentation with the implemented `/api/v1` resource API, document the native request and response field names for current control surfaces, and add route-shape coverage that rejects legacy RPC paths on the native router.

Files: `README.md`, `crates/raria-rpc/tests/native_api.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-rpc --test native_api native_api_router_does_not_mount_legacy_rpc_paths` passed. `cargo test -p raria-rpc --test native_api` passed with 35 tests. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: the README now lists the current native control routes, native request and response field names, native error envelope, and native event envelope shape without documenting JSON-RPC usage. Native API route-shape coverage proves the standalone native router rejects `/jsonrpc`, `/rpc`, and `/api/v1/jsonrpc` paths.

Remaining after completion: native hooks and lifecycle-event cleanup.

Next: Checkpoint 103.

### Checkpoint 103: Native Hook Lifecycle Cleanup

Status: complete

Scope: move daemon lifecycle hooks onto native task lifecycle names and native event delivery while preserving the useful hook behavior covered by session smoke tests.

Files: `crates/raria-cli/src/hooks.rs`, `crates/raria-cli/src/main.rs`, `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-core/src/config.rs`, `crates/raria-core/src/config_file.rs`

Validation: `cargo test -p raria-cli daemon_accepts_native_task_hook_names` passed. `cargo test -p raria-core apply_config_native_hook_scripts` passed. `cargo test -p raria-cli --test session_smoke daemon_runs_on_task_start_hook` passed. `cargo test -p raria-cli --test session_smoke daemon_runs_on_task_complete_hook` passed. `cargo test -p raria-cli --test session_smoke daemon_runs_on_task_fail_hook` passed. `cargo test -p raria-cli --test session_smoke` passed with 18 tests. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: daemon hooks now use native public CLI names `--on-task-start`, `--on-task-complete`, and `--on-task-fail`. The hook runner subscribes to the native event bus and dispatches from `TaskStarted`, `TaskCompleted`, and `TaskFailed` events using `TaskId` lookup instead of the legacy `DownloadEvent` bus and public GID lookup. Session smoke tests prove start, completion, and failure hooks receive native task identifiers.

Remaining after completion: strict native `raria.toml` hook configuration and legacy config parser cleanup.

Next: Checkpoint 104.

### Checkpoint 104: Strict Native Hook Configuration

Status: complete

Scope: move lifecycle hook configuration into strict native `raria.toml` and remove legacy hook names from key-value config and daemon CLI aliases.

Files: `crates/raria-core/src/native_config.rs`, `crates/raria-core/tests/native_config.rs`, `crates/raria-core/src/config_file.rs`, `crates/raria-cli/src/main.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-core --test native_config` passed with 8 tests. `cargo test -p raria-core config_file` passed with 49 matching unit tests and no filtered integration failures. `cargo test -p raria-cli daemon_rejects_legacy_download_hook_names` failed before alias removal and passed after implementation. `cargo test -p raria-cli daemon_accepts_native_task_hook_names` passed. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: strict native `[hooks]` now accepts `task_started`, `task_completed`, and `task_failed`, maps them into runtime task lifecycle hooks, and rejects legacy hook names as unknown fields. The legacy key-value parser no longer owns hook settings. The daemon CLI accepts only `--on-task-start`, `--on-task-complete`, and `--on-task-fail`; old `--on-download-*` aliases are rejected.

Remaining after completion: finish the hook runtime contract and remove obsolete hook wording outside the native lifecycle surface.

Next: Checkpoint 105.

### Checkpoint 105: Hook Runtime Contract Cleanup

Status: complete

Scope: route hook argument construction through the native task projection so lifecycle hooks depend on the native task contract instead of reading runtime `Job` fields directly.

Files: `crates/raria-cli/src/hooks.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-cli hook_context` passed with 2 tests. `cargo test -p raria-cli --test session_smoke daemon_runs_on_task_start_hook` passed. `cargo test -p raria-cli --test session_smoke daemon_runs_on_task_complete_hook` passed. `cargo test -p raria-cli --test session_smoke daemon_runs_on_task_fail_hook` passed.

Evidence: hook execution now builds `HookTaskContext` from `Engine::native_task_summary`, preserving native task id, file count, and output path as the hook argument contract. Focused tests cover multi-file BitTorrent file counts through the native projection and missing native task errors. Session smoke tests continue to prove start, completion, and failure hooks run against real daemon lifecycle events.

Remaining after completion: remove remaining aria2-shaped CLI aliases, option names, and legacy config parser behavior after equivalent native tests exist.

Next: Checkpoint 106.

### Checkpoint 106: Native API Port CLI Cleanup

Status: complete

Scope: remove the daemon `--rpc-port` compatibility alias and make daemon smoke helpers start and probe the native API through `--api-port` and `/api/v1/health`.

Files: `crates/raria-cli/src/main.rs`, `crates/raria-cli/src/daemon.rs`, `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-cli/tests/bt_tracker_smoke.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-cli daemon_rejects_legacy_rpc_port_name` passed. `cargo test -p raria-cli daemon_accepts_native_api_port_name` passed. `cargo test -p raria-cli --test session_smoke daemon_saves_session_when_native_save_session_is_called` passed. `cargo test -p raria-cli --test bt_tracker_smoke daemon_bt_tracker_option_announces_to_tracker_on_real_daemon_path` passed. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: the daemon CLI now accepts only `--api-port` for the control listener. Session and BitTorrent daemon smoke helpers use `--api-port` and native health probing instead of JSON-RPC readiness checks. The native save-session smoke fixture no longer carries `rpc-save` naming.

Remaining after completion: continue removing aria2-shaped CLI option names, JSON-RPC-specific daemon configuration, and the legacy key-value config parser after native replacements are covered.

Next: Checkpoint 107.

### Checkpoint 107: Retry Help Text Cleanup

Status: complete

Scope: remove aria2-specific retry wording from CLI help text and retry policy comments while preserving the existing retry behavior.

Files: `crates/raria-cli/src/main.rs`, `crates/raria-cli/src/executor_config.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-cli executor_config` passed with 6 tests. `cargo test -p raria-cli daemon_accepts_native_api_port_name` passed. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: retry help text now describes raria behavior directly, including unlimited retries when the maximum retry count is zero. The retry policy comment now describes native retry wait semantics instead of referencing aria2.

Remaining after completion: replace remaining aria2-shaped CLI option names and legacy config parser behavior after focused native coverage exists.

Next: Checkpoint 108.

### Checkpoint 108: Legacy RPC Config Key Rejection

Status: complete

Scope: remove JSON-RPC configuration ownership from the legacy key-value parser's strict path while preserving non-RPC downloader config behavior.

Files: `crates/raria-core/src/config_file.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-core strict_mode_rejects_legacy_rpc_keys` failed before implementation and passed after the rejection path was added. `cargo test -p raria-core config_file` passed with 47 matching unit tests and no filtered integration failures. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: strict key-value config parsing now rejects `rpc-listen-port`, `enable-rpc`, `rpc`, `rpc-secret`, and `rpc-allow-origin-all` with an explicit legacy RPC key error. The parser no longer mutates `rpc_listen_port`, `enable_rpc`, `rpc_secret`, or `rpc_allow_origin_all` from key-value config. The roundtrip config fixture no longer carries RPC keys.

Remaining after completion: continue replacing or deleting non-RPC legacy key-value config behavior after equivalent `raria.toml` coverage exists.

Next: Checkpoint 109.

### Checkpoint 109: Legacy RPC CLI Flag Removal

Status: complete

Scope: remove JSON-RPC auth and CORS flags from the daemon CLI while leaving the transitional JSON-RPC internals for the later RPC removal checkpoints.

Files: `crates/raria-cli/src/main.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-cli daemon_rejects_legacy_rpc_auth_flags` failed before implementation and passed after the CLI fields were removed. `cargo test -p raria-cli daemon_rejects_legacy_rpc_port_name` passed. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: `raria daemon` no longer accepts `--rpc-secret` or `--rpc-allow-origin-all`. The daemon CLI no longer maps JSON-RPC auth or CORS flags into `GlobalConfig`. Transitional `GlobalConfig` and `raria-rpc` fields remain internal migration debt for the dedicated JSON-RPC deletion checkpoints.

Remaining after completion: continue replacing aria2-shaped public CLI options and legacy config parser behavior after equivalent native coverage exists.

Next: Checkpoint 110.

### Checkpoint 110: BitTorrent Legacy Encryption Surface Removal

Status: complete

Scope: remove aria2 MSE/ARC4-facing BitTorrent encryption configuration from public CLI, legacy key-value config ownership, runtime forwarding, and compatibility tests.

Files: `crates/raria-cli/src/main.rs`, `crates/raria-cli/src/bt_runtime.rs`, `crates/raria-core/src/config.rs`, `crates/raria-core/src/config_file.rs`, `crates/raria-bt/src/service.rs`, `crates/raria-bt/tests/bt_smoke.rs`, `crates/raria-bt/tests/bt_gap_ledger.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-cli daemon_rejects_legacy_bt_crypto_flags` failed before implementation and passed after the daemon flags were removed. `cargo test -p raria-core strict_mode_rejects_legacy_bt_crypto_keys` failed before implementation and passed after strict key rejection was added. `cargo test -p raria-cli bt_service_config_forwards_piece_strategy` passed. `cargo test -p raria-bt bt_service_session_options_enable_fastresume_and_json_persistence` passed. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: `raria daemon` no longer accepts `--bt-require-crypto` or `--bt-min-crypto-level`. Strict key-value config parsing rejects `bt-require-crypto` and `bt-min-crypto-level`. `GlobalConfig` no longer stores `bt_require_crypto`, `bt_min_crypto_level`, or `BtMinCryptoLevel`. The CLI BitTorrent runtime no longer forwards a peer encryption policy. `raria-bt` no longer exposes the unused local peer encryption policy types that were retained only for compatibility with an unsupported MSE/PE path. Compatibility tests and gap-ledger entries that treated unsupported MSE/ARC4 behavior as an implementation target were deleted.

Remaining after completion: continue replacing aria2-shaped public CLI options and legacy config parser behavior after equivalent native coverage exists.

Next: Checkpoint 111.

### Checkpoint 111: Legacy Key-Value Config Parser Removal

Status: complete

Scope: delete the aria2-style key-value config parser and its legacy tests after confirming runtime configuration uses strict native `raria.toml`.

Files: `crates/raria-core/src/config_file.rs`, `crates/raria-core/src/lib.rs`, `crates/raria-core/tests/island_wiring.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-core conf_path_overrides_defaults` passed before deletion and returned no matching tests after the old parser tests were removed. `cargo test -p raria-core --test island_wiring` passed with 4 native wiring tests. `cargo test -p raria-core --test native_config` passed with 8 tests. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: `raria-core` no longer exports `config_file`. The aria2-style key-value parser module was deleted. Remaining `island_wiring` tests no longer import or exercise compatibility parsing, and native configuration coverage continues to prove `raria.toml` rejects legacy field names while converting native sections into runtime configuration.

Remaining after completion: remove remaining aria2-shaped CLI option names and continue native identity and persistence cleanup.

Next: Checkpoint 112.

### Checkpoint 112: Native Identity And Persistence Cleanup

Status: complete

Scope: remove deterministic `task_migration_` task identifiers and fallback decoding from native task rows, while preserving the temporary runtime `Gid` bridge only as an explicit private field until the native engine fully replaces `Job`.

Files: `crates/raria-core/src/native.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-core/src/scheduler.rs`, `crates/raria-core/src/lib.rs`, `crates/raria-rpc/src/methods.rs`, `docs/modernization/modernization-runbook.md`

Validation: `cargo test -p raria-core task_row_requires_explicit_runtime_bridge_for_job_restore` failed before implementation because `task_migration_0000000000000063` restored runtime `Gid(99)` through fallback decoding, then passed after removal. `cargo test -p raria-core save_session_persists_native_task_rows` failed before implementation because saved rows still used deterministic `task_migration_` IDs, then passed after native task IDs were persisted. `cargo test -p raria-core native_task_index_resolves_task_ids_and_runtime_job_ids` passed. `cargo test -p raria-core native_tasks_to_activate_returns_task_ids_without_stale_queue_entries` passed. `cargo test -p raria-core scheduler::tests::` passed with 17 tests. `cargo test -p raria-core engine::tests::add_uri_with_position_inserts_into_waiting_queue` passed. `cargo test -p raria-core engine::tests::change_position` passed with 3 tests. `cargo test -p raria-rpc add_uri_position` passed after deleting stale `add_torrent` method tests that no longer matched the registered RPC surface. `cargo test -p raria-rpc tell_active_includes_seeding_jobs` passed. `cargo check --workspace --locked` passed. `cargo fmt --all --check` passed. `git diff --check` passed.

Evidence: `TaskId::from_migration_gid`, `NativeTaskIndex::register_migration_gid`, scheduler GID queue wrappers, `waiting_queue` GID projection, and `NativeTaskRow::to_job_for_migration` fallback parsing were removed. `add_uri` now assigns opaque `TaskId::new()` when no caller-provided native task id exists. Scheduler queue operations now store and mutate native task ids directly, with runtime `Gid` activation resolved through the registry or private native task index. Native task rows must carry an explicit private `runtime_bridge_id` while `Job` remains the runtime model, so a crafted `task_migration_` string no longer recreates a runtime job id.

Remaining after completion: direct `Job` row persistence and private runtime bridge IDs remain until the native persistence schema owns restore end to end.

Next: Checkpoint 113.

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
