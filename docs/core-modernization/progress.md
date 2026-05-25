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

2026-05-25 CM-008 partial
Changed: Started the native task runtime boundary. Native pause, resume,
remove, and restart now resolve and mutate through TaskId-first paths instead
of delegating through the old Gid lifecycle wrappers. Native lifecycle
structured logs now use `task_id` fields.
Verified: `cargo test -p raria-core
native_lifecycle_log_fields_use_task_id_only -- --nocapture` passed. `cargo
test -p raria-core native_lifecycle_operations_publish_native_events --
--nocapture` passed. `cargo test -p raria-rpc --test native_api
task_detail_pause_and_resume_use_native_task_id -- --nocapture` passed. `cargo
test -p raria-rpc --test native_api
native_events_websocket_streams_native_lifecycle_events -- --nocapture`
passed. `cargo test -p raria-rpc --test native_api
task_remove_and_restart_are_native_actions -- --nocapture` passed. `cargo
check --workspace --locked` passed. `cargo fmt --all --check` passed.
Remaining: Continue CM-008 range activation.
Blocked: none.

2026-05-25 CM-008 partial
Changed: Moved daemon range activation context to TaskId-only ownership.
Range execution no longer stores runtime Gid in `RangeExecutionContext`; the
remaining Gid bridge is resolved inside the current private persistence and
event boundary.
Verified: `cargo test -p raria-cli --bin raria
interrupted_segment_persistence_does_not_create_legacy_rows -- --nocapture`
passed. `cargo test -p raria-cli --bin raria
range_structured_fields_use_native_task_id -- --nocapture` passed. `cargo
test -p raria-cli --bin raria
mirror_failover_publishes_source_failed_event_before_completion -- --nocapture`
passed. `cargo test -p raria-cli --test session_smoke
daemon_fails_over_to_next_mirror_when_first_mirror_fails -- --nocapture`
passed. `cargo check --workspace --locked` passed. `cargo fmt --all --check`
passed.
Remaining: Continue CM-008 BT activation.
Blocked: none.

2026-05-25 CM-008 partial
Changed: Moved daemon BT activation to TaskId entry. `run_bt_download` now
receives `TaskId` and resolves the current runtime Gid only inside the
remaining private Job and librqbit bridge.
Verified: `cargo test -p raria-cli --bin raria bt_cancel_handler --
--nocapture` passed with 2 matching tests. `cargo test -p raria-cli --bin
raria bt_service_config -- --nocapture` passed with 4 matching tests. `cargo
test -p raria-cli --bin raria
sync_bt_job_from_status_publishes_native_peer_and_tracker_events --
--nocapture` passed. `cargo test -p raria-cli --bin raria bt -- --nocapture`
passed with 24 matching tests. `cargo check --workspace --locked` passed.
`cargo fmt --all --check` passed.
Remaining: Continue CM-008 native mutation policy.
Blocked: none.

2026-05-25 CM-008 partial
Changed: Closed focused native mutation policy for this checkpoint. Native
transfer policy now updates rate limiters through TaskId, and existing native
mutation coverage verifies sources, trackers, seeding, queue position, BT file
selection, and native not-found errors.
Verified: `cargo test -p raria-core
native_runtime_helpers_manage_rate_limiter_and_segment_state -- --nocapture`
passed. `cargo test -p raria-rpc --test native_api
task_transfer_patch_updates_native_runtime_limits -- --nocapture` passed.
`cargo test -p raria-rpc --test native_api
task_sources_patch_replaces_native_range_sources -- --nocapture` passed.
`cargo test -p raria-rpc --test native_api
task_trackers_patch_updates_native_bt_trackers -- --nocapture` passed. `cargo
test -p raria-rpc --test native_api
task_bt_seeding_patch_updates_native_seed_policy -- --nocapture` passed.
`cargo test -p raria-rpc --test native_api
task_queue_patch_updates_native_waiting_position -- --nocapture` passed.
`cargo test -p raria-rpc --test native_api
task_files_patch_updates_native_bt_file_selection -- --nocapture` passed.
`cargo test -p raria-rpc --test native_api
task_mutation_routes_return_native_not_found_errors -- --nocapture` passed.
`cargo check --workspace --locked` passed. `cargo fmt --all --check` passed.
Remaining: Run CM-008 runtime closure checks.
Blocked: none.

2026-05-25 CM-008 verified
Changed: Closed the native task runtime checkpoint. Lifecycle, range
activation, BT activation, transfer policy mutation, and focused runtime
coverage now use TaskId-first ownership. Remaining Gid bridge debt is private
and assigned to native persistence and final legacy deletion checkpoints.
Verified: `cargo test -p raria-core native_runtime -- --nocapture` passed with
5 tests. `cargo test -p raria-core native_lifecycle -- --nocapture` passed
with 2 tests. `cargo test -p raria-cli --bin raria bt -- --nocapture` passed
with 24 tests. `cargo test -p raria-rpc --test native_api -- --nocapture`
passed with 36 tests. `cargo test -p raria-cli --bin raria
mirror_failover_publishes_source_failed_event_before_completion --
--nocapture` passed. Stale runtime bridge scan passed. `cargo check
--workspace --locked` passed.
Remaining: Start CM-009 versioned native persistence.
Blocked: none.

2026-05-25 CM-009 partial
Changed: Moved active session persistence to native rows. Store no longer
creates or exposes direct `jobs`, `segments`, or `job_options` session tables.
Native segment checkpoints now persist versioned native segment rows. Restore,
save-session, engine persistence, BT runtime persistence, and daemon segment
resume no longer use old Gid-keyed session tables.
Verified: `cargo test -p raria-core native_persist -- --nocapture` passed
with 10 tests. `cargo test -p raria-core persist -- --nocapture` passed with
28 core tests plus focused integration coverage. `cargo test -p raria-core
engine_restore -- --nocapture` passed with 7 tests. `cargo test -p raria-core
save_session -- --nocapture` passed with 4 tests. `cargo test -p raria-cli
--bin raria native_segment -- --nocapture` passed. `cargo test -p raria-cli
--bin raria bt -- --nocapture` passed with 24 tests. Stale old-table API scan
passed for production and test sources. `cargo check --workspace --locked`
passed.
Remaining: Run CM-009 closure validation and mark CM-009.5 when clean.
Blocked: none.

2026-05-25 CM-009 verified
Changed: Closed versioned native persistence. Native task and segment rows are
the active session truth. Direct `Job` rows, old Gid segment rows, raw
`JobOptions` rows, old Store APIs, and old persistence tests were removed.
Verified: Focused native persistence, restore, save-session, daemon native
segment, and BT tests passed. `cargo check --workspace --locked`,
`cargo fmt --all --check`, CSV validation, stale old-table API scan, and
`git diff --check` passed.
Remaining: Start CM-010 session save and crash recovery.
Blocked: none.

2026-05-25 CM-010 verified
Changed: Closed native session save and crash recovery. Range restart,
If-Range, preserved completed bytes, native segment row resume, BT fastresume
binding, BT backend restart restore, explicit save, periodic save, SIGUSR1
save, and terminal lifecycle recovery are verified. Added focused core restore
coverage for failed and removed terminal history states.
Verified: `cargo test -p raria-cli --test session_smoke
daemon_resume_after_restart_issues_range_request -- --nocapture` passed.
`cargo test -p raria-cli --test session_smoke
daemon_resume_after_restart_sends_if_range_when_etag_is_known --
--nocapture` passed. `cargo test -p raria-cli --test session_smoke
daemon_resume_after_restart_surfaces_non_zero_completed_length_before_completion
-- --nocapture` passed. `cargo test -p raria-cli --test native_api_smoke
daemon_resume_uses_native_segment_rows_after_restart -- --nocapture` passed.
`cargo test -p raria-cli --test bt_tracker_smoke
daemon_binds_bt_fastresume_state_to_native_session_path -- --nocapture`
passed. `cargo test -p raria-bt --test bt_smoke
bt_service_persists_fastresume_state_and_restores_progress_after_restart --
--nocapture` passed. `cargo test -p raria-cli --test session_smoke
daemon_periodically_saves_session_when_interval_is_enabled -- --nocapture`
passed. `cargo test -p raria-cli --test session_smoke
daemon_saves_session_when_native_save_session_is_called -- --nocapture`
passed. `cargo test -p raria-cli --test session_smoke
daemon_saves_session_when_sigusr1_is_received -- --nocapture` passed.
`cargo test -p raria-core engine_restore -- --nocapture` passed with 9
matching tests.
Remaining: Start CM-011 HTTP and HTTPS native transfer contract.
Blocked: none.

2026-05-25 CM-011 verified
Changed: Closed the HTTP and HTTPS native transfer contract. Request headers,
Basic auth, user agent, suggested filename, TLS/mTLS, proxy, no-proxy, cookie
load/save, netrc, conditional GET, If-Range, and resume guard behavior are
verified through focused local tests. Removed the legacy `.aria2` control-file
guard from single-download and daemon HTTP conditional/resume decisions.
Touched HTTP comments no longer describe retained behavior as aria2
compatibility.
Verified: `cargo test -p raria-cli --test single_download
single_download_sends_configured_user_agent -- --nocapture` passed. `cargo
test -p raria-cli --test single_download
single_download_sends_basic_auth_from_cli_flags -- --nocapture` passed.
`cargo test -p raria-cli --test single_download
single_download_sends_custom_header_from_cli -- --nocapture` passed. `cargo
test -p raria-cli --test single_download
single_download_uses_suggested_filename_when_out_is_not_provided --
--nocapture` passed. `cargo test -p raria-http --test http_config_smoke --
--nocapture` passed with 9 tests. `cargo test -p raria-cli --test
single_download single_download_presents_client_identity_for_mtls --
--nocapture` passed. `cargo test -p raria-cli --test single_download
single_download_writes_save_cookies_file -- --nocapture` passed. `cargo test
-p raria-cli --test single_download
single_download_uses_netrc_credentials_for_http_auth -- --nocapture` passed.
`cargo test -p raria-cli --test single_download
single_download_no_netrc_disables_netrc_credentials -- --nocapture` passed.
`cargo test -p raria-cli conditional_get -- --nocapture` passed. `cargo test
-p raria-cli --test single_download
single_download_conditional_get_ignores_legacy_control_file -- --nocapture`
passed. `cargo test -p raria-http --test if_range -- --nocapture` passed.
Remaining: Start CM-012 FTP FTPS SFTP and SCP decision.
Blocked: none.

2026-05-25 CM-012 verified
Changed: Closed the FTP, FTPS, SFTP, and SCP decision checkpoint. FTP/FTPS
remain suppaftp-backed. SFTP remains russh/russh-sftp-backed and is the
supported SSH-family file-transfer path. SCP is documented as a technical
limitation because current candidate crates do not justify adding a new
backend: `openssh` shells out to OpenSSH, `simple_ssh` is early 0.1.x, and
`fast-scp` is a CLI tool. Touched SFTP comments no longer describe retained
behavior as aria2 compatibility.
Verified: `cargo test -p raria-ftp --test ftp_smoke -- --nocapture` passed
with 3 tests. `cargo test -p raria-ftp --test ftps_smoke -- --nocapture`
passed. `cargo test -p raria-sftp --test sftp_smoke -- --nocapture` passed
with 5 tests. `cargo test -p raria-cli --test sftp_smoke -- --nocapture`
passed with 3 tests. `cargo test -p raria-cli --test single_download
single_download_supports_plain_ftp_urls -- --nocapture` passed. `cargo test
-p raria-cli --test single_download
single_download_supports_plain_ftp_urls_through_socks5_proxy -- --nocapture`
passed. `cargo test -p raria-cli --test single_download
single_download_supports_explicit_ftps_with_custom_ca -- --nocapture`
passed. `cargo info suppaftp --locked`, `cargo info russh --locked`, `cargo
info russh-sftp --locked`, `cargo info openssh --locked`, `cargo info
simple_ssh --locked`, and `cargo info fast-scp --locked` recorded dependency
freshness and SCP decision evidence.
Remaining: Start CM-013 multi-source adaptive transfers.
Blocked: none.

2026-05-25 CM-013 verified
Changed: Closed multi-source adaptive transfers. Daemon mirror failover now
discards the failed mirror's segment plan and checkpoint callback before
trying the next mirror, so fallback mirrors with different size or range
capabilities get a fresh resume-safe plan. Added a focused regression that
failed before the fix and passes after it.
Verified: `cargo test -p raria-cli --bin raria
mirror_failover_replans_segments_for_selected_source_capabilities --
--nocapture` failed before the fix and passed after it. `cargo test -p
raria-cli --bin raria plan_download_segments_uses_selected_source_health --
--nocapture` passed. `cargo test -p raria-cli --bin raria
mirror_failover_publishes_source_failed_event_before_completion --
--nocapture` passed. `cargo test -p raria-cli --test session_smoke
daemon_fails_over_to_next_mirror_when_first_mirror_fails -- --nocapture`
passed. `cargo test -p raria-cli --bin raria native_segment -- --nocapture`
passed. `cargo test -p raria-core native_source_selection -- --nocapture`
passed. `cargo test -p raria-core native_segment_planning -- --nocapture`
passed. `cargo test -p raria-core
native_runtime_helpers_manage_rate_limiter_and_segment_state -- --nocapture`
passed.
Remaining: Start CM-014 integrity and disk policy.
Blocked: none.
