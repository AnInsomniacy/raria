# Core Modernization Progress

This file is the compact chronological evidence trail for the raria
core-modernization tracker. Keep entries checkpoint-sized. Do not record raw
logs, packet captures, generated reports, local public-network data,
temporary downloads, local caches, API payloads, or conversation text.

Use this format:

```text
YYYY-MM-DD CM-XXX status
Changed: concise tracker, code, or behavior summary.
Verified: exact final command and result, or documentation-only reason.
Remaining: next concrete gap.
Blocked: none, or exact blocker.
```

## Current Baseline

The previous modernization run reached working multi-source failover and left
adaptive segmented transfer behavior as the next known runtime gap. raria has
working HTTP/HTTPS, FTP/FTPS, SFTP, Metalink, BitTorrent, segmented downloads,
retry, resume, native API routes, native WebSocket events, redb persistence,
structured logs, and daemon smoke tests.

The branch is not complete. Major debt remains in JSON-RPC, `Gid`, `Job`,
aria2-shaped option names, compatibility terminology, parity tests, direct
`Job` row persistence, old segment fallback tables, and old documentation.

The new tracker starts at CM-001. The first implementation-bearing checkpoint
after tracker rebuild is CM-003, Native Surface Audit.

## Log

2026-05-25 CM-001 verified
Changed: Created the active tracker under `docs/core-modernization` with a
native-only goal contract, deletion policy, library policy, restrained test
policy, roadmap, capability ledger, dependency ledger, source evidence policy,
progress log, and small checkpoint files. Removed the old docs files so
`docs` contains only the active tracker.
Verified: CSV parser validation passed for 24 files. `git diff --check`
passed. Stale old-runbook reference scan passed.
Remaining: Start CM-002 dependency policy verification.
Blocked: none.

2026-05-25 CM-006 partial
Changed: Renamed visible download and daemon CLI flags to native names without
aliases. Updated focused CLI tests, smoke test invocations, and README command
examples. The help scan now checks retained flags and value placeholders and
rejects deleted legacy names.
Verified: `cargo test -p raria-cli --bin raria -- --nocapture` passed with
68 tests. `cargo test -p raria-cli --test single_download -- --nocapture`
passed with 23 tests. `cargo test -p raria-cli --test session_smoke
daemon_loads_jobs_from_input_file_on_startup -- --nocapture` passed. `cargo
test -p raria-cli --test native_api_smoke
daemon_flag_detaches_process_and_keeps_native_api_alive -- --nocapture`
passed. `cargo check --workspace --locked` passed. CSV parser validation
passed for 25 files. `git diff --check` passed.
Remaining: Continue reviewing remaining runtime config names and raria.toml
coverage.
Blocked: none.

2026-05-25 CM-001 alignment update
Changed: Aligned the tracker with aria2-next core-modernization and
libtorrent migration evidence without inheriting aria2-next compatibility
surfaces. Added explicit coverage for future native client integration, shell
completion, SCP decision, BitTorrent metadata-only behavior, duplicate
info-hash policy, PEX policy, ordinary transfer stall watchdogs, environment
proxy policy, native product docs and release closure, and final smoke
evidence.
Verified: CSV parser validation passed for 25 files. `git diff --check`
passed.
Remaining: Start CM-002 dependency policy verification.
Blocked: none.

2026-05-25 CM-002 verified
Changed: Centralized repeated direct dependency versions in workspace
dependencies for FTP, SFTP, BitTorrent, checksum, TLS helper, platform, and
focused test crates. Rebuilt `dependency-ledger.csv` with locked versions,
target ownership, accepted transitive duplicate boundaries, rejected dependency
directions, and upgrade policy. Marked CM-002 rows verified.
Verified: `cargo check --workspace --locked` passed. CSV parser validation
passed for 25 files. `git diff --check` passed.
Remaining: Start CM-003 native surface audit and deletion map.
Blocked: none.

2026-05-25 CM-003 verified
Changed: Added `native-surface-audit.md` as the native API, event, CLI,
configuration, JSON-RPC, test, documentation, and stale-surface deletion map.
Mapped retained `/api/v1` resources, native event types, legacy JSON-RPC
removal targets, transitional config names, compatibility test deletion
candidates, and reproducible stale-surface scans.
Verified: CSV parser validation passed for 25 files. `git diff --check`
passed.
Remaining: Start CM-004 core ownership audit.
Blocked: none.

2026-05-25 CM-004 verified
Changed: Added `core-ownership-audit.md` with the TaskId/Gid bridge map,
Job-driven runtime ownership, native scheduler state, redb table decisions,
event ownership, BitTorrent librqbit boundary, and refactor order for CM-005
through CM-020.
Verified: CSV parser validation passed for 25 files. `git diff --check`
passed.
Remaining: Start CM-005 native API and event stream closure.
Blocked: none.

2026-05-25 CM-005 partial
Changed: Changed the daemon to start `start_native_api_server` directly, so
the running product listener exposes `/api/v1` and `/api/v1/events` without
mounting `/jsonrpc`. Updated raria-rpc crate docs to describe the native
contract first. Added a daemon smoke assertion that `/jsonrpc` is not exposed.
Verified: `cargo test -p raria-rpc --test native_api -- --nocapture` passed
with 36 tests. `cargo test -p raria-cli --test native_api_smoke daemon_ --
--nocapture` passed with 28 tests. `cargo check --workspace --locked` passed.
Remaining: CM-006 native CLI and configuration closure, then CM-020 deletes
remaining JSON-RPC modules and compatibility tests.
Blocked: none.

2026-05-25 CM-006 partial
Changed: Removed `rpc_listen_port` and `enable_rpc` from `GlobalConfig`,
daemon startup, native `raria.toml` conversion, and focused assertions. README
now documents `[api].listen_addr` and describes the daemon listener as native
API only. Removed stale aria2 option comments from the touched runtime config
fields.
Verified: `cargo test -p raria-core --test native_config -- --nocapture`
passed with 8 tests. `cargo test -p raria-rpc --test native_api
config_endpoint_returns_native_runtime_projection -- --nocapture` passed.
`cargo check --workspace --locked` passed. CSV parser validation passed for
25 files. `git diff --check` passed.
Remaining: Continue CM-006 stale CLI/config review. Remaining legacy-shaped
fields such as `rpc_secret`, `rpc_allow_origin_all`, task-level `dir`,
task-level `out`, and selected BitTorrent option names need native
replacement or deletion conditions.
Blocked: none.

2026-05-25 CM-006 partial
Changed: Renamed retained runtime config fields from aria2-shaped names to
native raria terms across CLI wiring, daemon wiring, protocol backend config,
native API config projection, task-file parsing, and focused tests. Added a
serialization guard that rejects deleted `GlobalConfig` field names.
Verified: `cargo test -p raria-core
global_config_serialization_uses_native_field_names -- --nocapture` passed.
`cargo test -p raria-core --test input_file -- --nocapture` passed with 8
tests. `cargo test -p raria-cli --bin raria executor_config -- --nocapture`
passed with 6 tests. `cargo test -p raria-core --test native_config --
--nocapture` passed with 8 tests. `cargo test -p raria-rpc --test native_api
config_endpoint_returns_native_runtime_projection -- --nocapture` passed.
`cargo test -p raria-http --test http_config_smoke -- --nocapture` passed
with 9 tests. `cargo test -p raria-ftp --test ftp_smoke -- --nocapture`
passed with 3 tests. `cargo test -p raria-sftp --test sftp_smoke --
--nocapture` passed with 5 tests. `cargo test -p raria-core --test
island_wiring -- --nocapture` passed with 4 tests.
Remaining: CM-006 still needs final stale config/help scans, tracker updates,
and a fresh `cargo check --workspace --locked` before the next checkpoint.
Blocked: none.

2026-05-25 CM-006 verified
Changed: Closed native CLI and configuration ownership for this checkpoint.
The public CLI uses native flags, strict `raria.toml` rejects unknown and
legacy keys, retained runtime config fields use native names, and stale
task-file/proxy comments were removed. Remaining `dir/out`, persistence
fixture, JSON-RPC auth, and BitTorrent policy debt is assigned to later
checkpoints.
Verified: `cargo fmt --all --check` passed. `cargo check --workspace
--locked` passed. `cargo test -p raria-cli --bin raria -- --nocapture`
passed with 68 tests. `cargo test -p raria-core --test native_config --
--nocapture` passed with 8 tests. `cargo test -p raria-rpc --test
native_api config_endpoint_returns_native_runtime_projection -- --nocapture`
passed. CLI help scan found retained native flags and no deleted legacy
aliases. CSV parser validation passed for 25 files. `git diff --check`
passed.
Remaining: Start CM-007 TaskId ownership after final validation.
Blocked: none.

2026-05-25 CM-007 partial
Changed: Moved cancellation ownership from runtime `Gid` keys to native
`TaskId` keys. Engine restore, submit, restart, pause, resume, remove,
activate, complete, fail, shutdown, and force-remove paths now register or
cancel tokens through task identifiers while the runtime bridge remains
private.
Verified: `cargo test -p raria-core cancel -- --nocapture` passed with 17
matching tests. `cargo test -p raria-core activate_job -- --nocapture`
passed. `cargo check --workspace --locked` passed.
Remaining: Continue CM-007 scheduler lookup and public `Gid` projection
cleanup.
Blocked: none.

2026-05-25 CM-007 partial
Changed: Removed the scheduler `jobs_to_activate` runtime-id path and the
engine `activatable_jobs` wrapper. Scheduler and daemon activation now use
`TaskId` as the activation list, with `Gid` kept only inside the current
runtime bridge after task activation.
Verified: `cargo test -p raria-core scheduler -- --nocapture` passed with
17 matching tests. `cargo test -p raria-core activatable_native_tasks --
--nocapture` passed. `cargo check --workspace --locked` passed.
Remaining: Continue CM-007 public `Gid` projection cleanup.
Blocked: none.

2026-05-25 CM-007 partial
Changed: Removed `gid` from structured lifecycle log fields that are part of
the native product surface. Core logging tests, daemon range log fields, and
BT lifecycle smoke now require `task_id` and reject `gid`. Existing native API
tests continue to assert `gid` is absent from public JSON responses.
Verified: `cargo test -p raria-core logging -- --nocapture` passed with 3
matching tests. `cargo test -p raria-cli --bin raria
range_structured_fields_use_native_task_id -- --nocapture` passed. `cargo
test -p raria-cli --test bt_tracker_smoke
daemon_log_file_contains_structured_bt_lifecycle_events -- --nocapture`
passed. `cargo check --workspace --locked` passed.
Remaining: Continue CM-007 migration helper cleanup and identity stale scans.
Blocked: none.

2026-05-25 CM-007 partial
Changed: Removed the duplicate task-id index and the migration task-id
registration helper. TaskId lookup now uses `JobRegistry.by_task_id`; native
row and summary adapters use runtime-job terminology; TaskId parsing accepts
only generated opaque native ids. Old-prefix assertions were replaced with
native-format checks.
Verified: `cargo test -p raria-core
native_task_id_lookup_uses_registry_task_index -- --nocapture` passed. Stale
migration helper scan found no matches outside tracker history.
Remaining: Run CM-007.5 identity closure checks and record private runtime
bridge debt for CM-008 and CM-009.
Blocked: none.

2026-05-25 CM-007 verified
Changed: Closed TaskId ownership for this checkpoint. Public identity uses
opaque native TaskId values; cancellation, queue activation, API lookups,
structured logs, and runtime helper tests no longer rely on migration task-id
helpers. The private runtime Gid bridge remains assigned to CM-008 and CM-009.
Verified: `cargo test -p raria-core task_id -- --nocapture` passed with 13
matching tests. `cargo test -p raria-rpc --test native_api registry_task_ids
-- --nocapture` passed with 2 matching tests. `cargo test -p raria-rpc --test
native_api task_creation_files_and_sources_are_native_resources --
--nocapture` passed. `cargo test -p raria-rpc --test native_api
task_created_event_uses_created_native_task_id -- --nocapture` passed. `cargo
test -p raria-cli --bin raria range_structured_fields_use_native_task_id --
--nocapture` passed. `cargo test -p raria-cli --bin raria bt -- --nocapture`
passed with 24 matching tests. `cargo test -p raria-cli --test
native_api_smoke daemon_exposes_native_api_endpoints -- --nocapture` passed.
`cargo test -p raria-cli --test session_smoke
daemon_restores_saved_job_after_restart -- --nocapture` passed. `cargo check
--workspace --locked` passed. `cargo fmt --all --check` passed. CSV validation
passed for 25 files. `git diff --check` passed. Stale migration helper scan
passed.
Remaining: Start CM-008 native task runtime model.
Blocked: none.
