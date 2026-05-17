# raria Modernization Progress Log

This log tracks checkpoints for the long-running modernization goal. It is intentionally concise and evidence-oriented.

## Checkpoint 96: Native Session Smoke Shutdown and Save

Status: complete

Date: 2026-05-17

Scope completed:

- Migrated the shared session smoke graceful shutdown helper from `aria2.shutdown` to `POST /api/v1/daemon/shutdown`.
- Added a native session smoke helper for protected `/api/v1` POST routes.
- Migrated the explicit daemon session-save smoke from `aria2.saveSession` to `POST /api/v1/session/save`.
- Verified native shutdown still preserves daemon restart and range resume behavior.

Current conclusion:

Session daemon smoke coverage no longer depends on JSON-RPC for graceful process shutdown or explicit session save. The same tests still use JSON-RPC for remaining task creation and status polling until those range/status paths are fully migrated to native task resources.

Validation:

- `cargo test -p raria-cli --test session_smoke daemon_saves_session_when_native_save_session_is_called`
- `cargo test -p raria-cli --test session_smoke daemon_resume_after_restart_issues_range_request`

Next checkpoint:

Migrate session smoke task creation and status polling from `aria2.addUri` and `aria2.tellStatus` to `/api/v1/tasks`.

## Checkpoint 95: Native Daemon Shutdown API

Status: complete

Date: 2026-05-17

Scope completed:

- Added `POST /api/v1/daemon/shutdown` as the raria-native graceful daemon shutdown route.
- Added native API contract coverage proving the shutdown response is a native envelope and cancels the engine shutdown token.
- Extended bearer-token contract coverage so unauthenticated shutdown requests are rejected.
- Added daemon smoke coverage proving native shutdown stops the process without JSON-RPC fields.
- Migrated BitTorrent tracker, fastresume, DHT-shutdown, and structured-log smoke shutdown paths from `aria2.shutdown` to `/api/v1/daemon/shutdown`.

Current conclusion:

Daemon lifecycle control now has a native shutdown endpoint with package-level and real-process smoke coverage. BitTorrent daemon smoke tests no longer rely on `aria2.shutdown` for graceful process exit, while the legacy JSON-RPC shutdown method remains only as part of the broader unfinished RPC surface removal.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_shutdown_stops_daemon_without_json_rpc`
- `cargo test -p raria-cli --test bt_tracker_smoke`
- `cargo test -p raria-rpc --test native_api daemon_shutdown_endpoint_uses_native_api_envelope`
- `cargo test -p raria-rpc --test native_api native_api_uses_bearer_token_auth_when_configured`

Next checkpoint:

Continue replacing remaining JSON-RPC daemon smoke controls with `/api/v1`, starting with range/session tests that still use `aria2.addUri`, `tellStatus`, `saveSession`, or `shutdown`.

## Checkpoint 94: Native BitTorrent Add Surface Cutover

Status: complete

Date: 2026-05-16

Scope completed:

- Removed the `aria2.addTorrent` JSON-RPC handler from the legacy RPC surface.
- Removed the old JSON-RPC BitTorrent dispatch test module.
- Added a legacy-surface contract proving `aria2.addTorrent` is no longer registered.
- Migrated daemon BitTorrent tracker, fastresume, DHT-shutdown, and structured-log smoke setup to create BT tasks through `/api/v1/tasks`.
- Removed duplicated RPC peer smoke coverage now covered by native BT peer/resource smoke.

Current conclusion:

BitTorrent task creation now uses the native task creation surface for daemon smoke coverage. The old `aria2.addTorrent` method is no longer registered, while native API tests continue to cover torrent bytes, torrent files, trackers, peers, WebSeed, file selection, seeding policy, fastresume binding, and DHT shutdown persistence.

Validation:

- `cargo test -p raria-rpc --test legacy_surface`
- `cargo test -p raria-rpc --test options_parity`
- `cargo test -p raria-cli --test bt_tracker_smoke`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_exposes_live_bt_metadata_peers_and_trackers`

Next checkpoint:

Continue removing the remaining `aria2.addUri` and JSON-RPC status/control dependencies after native range, session, and shutdown flows have equivalent coverage.

## Checkpoint 93: Native Metalink Control Surface Cutover

Status: complete

Date: 2026-05-16

Scope completed:

- Added native daemon smoke coverage for Metalink mirror failover after transfer failure.
- Added native daemon smoke coverage for Metalink mirror failover after checksum mismatch.
- Added native daemon smoke coverage for Metalink piece checksum failure.
- Removed the `aria2.addMetalink` JSON-RPC handler from the legacy RPC surface.
- Removed the old JSON-RPC Metalink dispatch test module after native coverage replaced it.
- Added a legacy-surface contract proving `aria2.addMetalink` is no longer registered.

Current conclusion:

Metalink task creation now enters through `/api/v1/tasks` instead of the aria2-shaped JSON-RPC method. The parser, normalizer, runtime metadata projection, mirror failover, whole-file checksum, and piece checksum daemon paths have native control-surface evidence.

Validation:

- `cargo test -p raria-rpc --test legacy_surface legacy_add_metalink_is_not_registered`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_metalink`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_enforces_metalink_piece_checksum_failure`

Next checkpoint:

Continue removing remaining JSON-RPC-dependent protocol smoke coverage, prioritizing BitTorrent and daemon session/control paths that still lack native-only proof.

## Checkpoint 92: Daemon Metalink Checksum Enforcement

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon smoke coverage for checksum failure from a native Metalink task.
- Verified a Metalink-provided whole-file checksum is enforced at terminal runtime verification.
- Preserved checksum failure detail in native source health after task failure.
- Added focused engine coverage for terminal native task failure exposing source error context.

Current conclusion:

Native Metalink checksum metadata now has daemon-path enforcement evidence, not only API projection coverage. Terminal integrity failures are visible through the raria-native task summary source health model.

Validation:

- `cargo test -p raria-core fail_native_task_records_terminal_source_error_when_missing`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_enforces_metalink_checksum_failure`

Next checkpoint:

Demote duplicated JSON-RPC Metalink assertions after confirming native API daemon coverage fully replaces them, then continue removing the old Metalink public surface.

## Checkpoint 91: Native Metalink Contract Coverage

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API coverage for Metalink whole-file checksum selection.
- Added native API coverage for Metalink expected size and piece checksum projection.
- Added native API coverage for invalid Metalink base64 returning a native error envelope.
- Added native API coverage for `metalink.path` task creation.
- Re-ran the native Metalink task creation contract tests.

Current conclusion:

The main Metalink behavior previously anchored only in `aria2.addMetalink` tests now has direct `/api/v1/tasks` coverage. The remaining JSON-RPC tests are increasingly migration scaffolding rather than the only proof of behavior.

Validation:

- `cargo test -p raria-rpc --test native_api task_creation_metalink`

Next checkpoint:

Demote or remove duplicated JSON-RPC Metalink assertions after confirming native coverage fully replaces them, then continue toward removing the JSON-RPC Metalink public surface.

## Checkpoint 90: Shared Native Metalink Task Helper

Status: complete

Date: 2026-05-16

Scope completed:

- Extracted shared raria-native Metalink helpers for XML parsing, runtime-config normalization, torrent metadata source detection, and task metadata projection.
- Moved both `/api/v1/tasks` Metalink creation and the migration JSON-RPC `aria2.addMetalink` path onto the shared helper.
- Preserved JSON-RPC-specific option mutation and lightweight legacy relation fields outside the shared helper.
- Verified native API and JSON-RPC Metalink behavior stayed stable after the extraction.

Current conclusion:

Metalink behavior now has one implementation core inside `raria-rpc`, which lowers the risk of drift while the native API replaces the old JSON-RPC surface.

Validation:

- `cargo test -p raria-rpc --test metalink_dispatch`
- `cargo test -p raria-rpc --test native_api task_creation_`
- `cargo check -p raria-rpc --locked`

Next checkpoint:

Continue migrating JSON-RPC-only Metalink assertions into native API tests, then remove the JSON-RPC Metalink method once native daemon coverage is equivalent.

## Checkpoint 89: Daemon Native Metalink Creation Smoke

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon smoke coverage for native `/api/v1/tasks` Metalink creation.
- Verified a multi-file Metalink document expands into native task summaries through the daemon.
- Verified both created range tasks reach the native completed lifecycle.
- Verified output bytes are written correctly for each Metalink file.

Current conclusion:

Native API Metalink creation now has daemon-path proof, not only API contract tests. This reduces dependence on the legacy JSON-RPC `aria2.addMetalink` path for Metalink behavior validation.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_creates_and_completes_metalink_tasks`

Next checkpoint:

Move remaining Metalink assertions from JSON-RPC-only tests into native API and daemon coverage, then demote or remove the old `aria2.addMetalink` migration surface when equivalent native coverage exists.

## Checkpoint 88: Native API Metalink Task Creation

Status: complete

Date: 2026-05-16

Scope completed:

- Added `/api/v1/tasks` support for native Metalink creation from `metalink.bytesBase64` and `metalink.path`.
- Returned a native `tasks` collection instead of a legacy GID list for Metalink expansion.
- Reused raria-native Metalink mirror preferences from runtime configuration.
- Preserved checksum, piece checksum, expected size, and metadata-source projection on created tasks.
- Routed Metalink torrent metaurls through the native BT metadata helper with Metalink mirrors as WebSeed URIs.
- Added native API contract coverage for multi-file Metalink creation and torrent metaurl BT creation.

Current conclusion:

Metalink task creation no longer depends only on the legacy JSON-RPC `aria2.addMetalink` surface. The native API can now accept Metalink bytes or files and create native task summaries. The old JSON-RPC path remains as migration scaffolding until daemon and client coverage fully move over.

Validation:

- `cargo test -p raria-rpc --test native_api task_creation_`
- `cargo test -p raria-rpc --test native_api task_creation_metalink_bytes_creates_native_tasks`
- `cargo test -p raria-rpc --test native_api task_creation_metalink_torrent_metaurl_creates_native_bt_task`
- `cargo test -p raria-rpc --test metalink_dispatch`

Next checkpoint:

Add daemon smoke coverage for native API Metalink creation, then remove or demote remaining Metalink behavior tests from JSON-RPC once native coverage is equivalent.

## Checkpoint 87: Native Metalink Mirror Preferences in raria.toml

Status: complete

Date: 2026-05-16

Scope completed:

- Added strict native `[metalink]` configuration fields for preferred locations, preferred protocol, and unique protocol selection.
- Carried Metalink preferences into the runtime configuration.
- Exposed the active Metalink preferences through `/api/v1/config`.
- Wired Metalink dispatch to use the runtime preferences instead of default-only normalization.
- Added config, native API, and dispatch coverage for the new path.

Current conclusion:

Metalink mirror preference filtering is now user-controllable through raria-native configuration and visible through the native API. This keeps the surface native and avoids aria2 option names.

Validation:

- `cargo test -p raria-core --test native_config`
- `cargo test -p raria-rpc --test native_api config_endpoint_returns_native_runtime_projection`
- `cargo test -p raria-rpc --test metalink_dispatch add_metalink_uses_native_mirror_preferences_from_runtime_config`

Next checkpoint:

Replace lightweight Metalink multi-file relation fields with a native task graph model, or migrate Metalink task creation from the legacy JSON-RPC surface to `/api/v1`.

## Checkpoint 86: Metalink Native Mirror Preference Filters

Status: complete

Date: 2026-05-16

Scope completed:

- Re-read aria2 Metalink resource filtering evidence for location preference, protocol preference, unique-protocol handling, and priority sorting.
- Added raria-native normalizer options for preferred mirror locations, preferred protocol, and one-source-per-protocol selection.
- Kept the default behavior compatible with raria's existing priority-only normalization.
- Added focused Metalink normalizer tests for each preference rule.

Current conclusion:

Metalink mirror filtering now has native data-plane support. This does not add aria2 option compatibility; API and configuration surfaces still need native fields before users can control these preferences outside tests.

Validation:

- `cargo test -p raria-metalink normalize_`
- `cargo test -p raria-metalink`
- `cargo test -p raria-rpc --test metalink_dispatch`
- `cargo check -p raria-metalink --locked`

Next checkpoint:

Expose Metalink mirror preferences through raria-native configuration or continue replacing lightweight Metalink multi-file relations with a native task graph model.

## Checkpoint 85: Remote Torrent Metadata Fetch for Metalink Graphs

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon smoke coverage for a native BT task created from a remote HTTP `.torrent` metadata source.
- Added runtime detection that keeps local `.torrent` files on the file path flow while fetching HTTP/HTTPS `.torrent` metadata as bytes.
- Fed fetched torrent bytes into the existing BT runtime and WebSeed pre-download path.
- Verified the task reaches native seeding lifecycle from remote metadata plus WebSeed payload.

Current conclusion:

Remote HTTP/HTTPS torrent metadata sources can now execute through the native BT runtime. This removes the main runtime blocker introduced by Metalink torrent metaurl dispatch. The remaining Metalink graph work is native multi-file graph modeling and broader source-derived Metalink filters.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_fetches_remote_torrent_metadata_sources`
- `cargo test -p raria-cli bt_runtime::tests::remote_torrent_metadata_detection_is_limited_to_http_torrent_uris`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_accepts_torrent_file_sources`

Next checkpoint:

Replace lightweight Metalink multi-file relation fields with the native task graph model, or continue filling Metalink location/protocol filter behavior from aria2 source evidence.

## Checkpoint 84: Metalink Torrent Metaurl Native BT Dispatch

Status: complete

Date: 2026-05-16

Scope completed:

- Added a failing RPC dispatch test proving Metalink `metaurl mediatype="torrent"` must create a BT task rather than a plain range task.
- Wired Metalink dispatch to choose the best torrent metadata source and submit it through the native BT metadata task helper.
- Preserved Metalink HTTP mirrors as BitTorrent WebSeed URIs on the created task.
- Kept Metalink metadata source records on the job for native persistence and inspection.
- Left ordinary Metalink files on the existing range dispatch path.

Current conclusion:

Metalink torrent metaurls now enter the native BitTorrent task path during dispatch. This completes the first executable hybrid graph step. Remote HTTP `.torrent` metadata still needs runtime fetching before full daemon-level hybrid execution can be claimed.

Validation:

- `cargo test -p raria-rpc --test metalink_dispatch add_metalink_dispatches_torrent_metaurl_as_bt_task_with_webseed_mirror`
- `cargo test -p raria-rpc --test metalink_dispatch`

Next checkpoint:

Fetch remote torrent metadata sources into the BT runtime path, or move the Metalink multi-file relation fields into the native task graph model.

## Checkpoint 83: Metalink Torrent Metaurl Source Graph Seed

Status: complete

Date: 2026-05-16

Scope completed:

- Re-read aria2 Metalink v4 `metaurl` parsing and grouping tests.
- Added explicit Metalink `metaurl` parsing instead of treating metadata URLs as ordinary download mirrors.
- Added normalized Metalink metadata sources with media type, priority, and optional name.
- Preserved torrent metaurls on Metalink-created jobs through `JobOptions::metalink_metadata_sources`.
- Added a native engine helper that creates a BT task from a Metalink torrent metadata source and carries Metalink mirrors as WebSeed URIs.
- Added focused parser, normalizer, and RPC dispatch coverage for torrent metaurls.

Current conclusion:

raria now has a native seed for Metalink metadata source graphs. Torrent metaurls are no longer mixed into HTTP/FTP mirror lists, and the engine can represent the BT side of a hybrid Metalink task. Full hybrid execution still needs Metalink dispatch to submit that native BT graph path.

Validation:

- `cargo fmt --all --check`
- `cargo test -p raria-metalink metaurl`
- `cargo test -p raria-rpc --test metalink_dispatch add_metalink_preserves_torrent_metaurl_as_metadata_source`
- `cargo test -p raria-core native_bt_task_from_metalink_metadata_source_carries_webseed_mirrors`
- `cargo check --workspace --locked`

Next checkpoint:

Wire Metalink dispatch to submit native BT graph tasks when torrent metadata sources are present, or replace the remaining lightweight multi-file relation fields with a native graph model.

## Checkpoint 82: Source-Health-Aware Segment Planning

Status: complete

Date: 2026-05-16

Scope completed:

- Re-read aria2 split, min-split-size, server-stat, and adaptive selector behavior as the source reference.
- Extended native segment planning input with the selected source URI.
- Fed daemon range mirror selection into native segment planning.
- Reduced planned range concurrency for degraded or failed sources instead of blindly using the static split target.
- Added core and daemon coverage for source-health-aware segment planning.

Current conclusion:

Adaptive segmented downloads now have a native runtime signal from mirror health. This is a small modern behavior slice, not aria2 option compatibility. The remaining work is live segment rebalancing while a transfer is already running.

Validation:

- `cargo fmt --all --check`
- `cargo test -p raria-core native_segment_planning_reduces_connections_for_degraded_source`
- `cargo test -p raria-cli daemon::tests::plan_download_segments_uses_selected_source_health`
- `cargo check --workspace --locked`

Next checkpoint:

Continue with live range adaptation or move to the next matrix gap with higher completion leverage.

## Checkpoint 81: Native Source Health Persistence

Status: complete

Date: 2026-05-16

Scope completed:

- Added native task row persistence for source health.
- Preserved source health through `save_session()` and native-row restore.
- Kept older native task rows readable through serde defaults for the new `sourceHealth` field.
- Extended native task row tests so source health is carried from the runtime job model into the versioned row and back.

Current conclusion:

Source health now survives daemon restart through raria-native persistence. This keeps the mirror-health work native and avoids aria2 server-stat file compatibility.

Validation:

- `cargo fmt --all --check`
- `cargo test -p raria-core save_session_and_restore_preserve_native_source_health`
- `cargo test -p raria-core task_row_`
- `cargo check --workspace --locked`

Next checkpoint:

Move from per-source scoring to richer adaptive segmented behavior, or continue retiring direct `Job` row reliance from native persistence depending on the next highest-risk matrix gap.

## Checkpoint 80: Native Source Health and Mirror Scoring

Status: complete

Date: 2026-05-16

Scope completed:

- Re-read aria2 mirror-health behavior in `ServerStat`, `ServerStatMan`, `AdaptiveURISelector`, the manual server-stat options, and focused aria2 tests.
- Added a raria-native source health projection with `state`, `failureCount`, `lastError`, `lastDownloadBytesPerSecond`, and `score`.
- Recorded native source failures and successes through engine helpers.
- Updated native source selection so unobserved mirrors are tested first, then the highest-scoring unattempted source is preferred.
- Exposed source health through `/api/v1/tasks/{taskId}/sources`.
- Recorded successful daemon range mirror completion into native source health.

Current conclusion:

raria now has a native mirror health model and an adaptive source-selection anchor. This intentionally does not preserve aria2 server-stat file compatibility. The remaining work is persistence of source health across restart and richer adaptive segment rebalancing.

Validation:

- `cargo test -p raria-core native_source`
- `cargo test -p raria-rpc --test native_api task_sources_get_projects_native_source_health`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`

Next checkpoint:

Persist native source health into the versioned raria store and validate restore behavior without aria2 server-stat files.

## Checkpoint 1: Source Audit Baseline

Status: complete

Date: 2026-05-13

Scope completed:

- Confirmed work is on the current `main` branch.
- Enumerated raria source, documentation, and test files while excluding `.git` and `target`.
- Enumerated aria2 source, manual, and tests as the feature reference.
- Corrected the initial aria2 file scan: it accidentally matched dependency documentation under `deps/`; dependency source and dependency documentation are excluded from the audit.
- Extracted aria2 manual sections, 197 documented CLI options, 35 documented aria2 RPC and notification method names, and the aria2 test inventory as feature-discovery inputs.
- Confirmed raria currently still exposes aria2-style JSON-RPC, aria2-style config parsing, aria2 GID formatting, and aria2 parity tests. These are incompatible with the modernization target and must be replaced or reclassified.

Current conclusion:

raria is a real downloader implementation, but it is not yet a full modern aria2 replacement under the new goal. The largest gaps are the public control surface, configuration format, versioned native persistence model, source-derived feature matrix coverage, and modern capability verification.

Next checkpoint:

Refine the feature matrix from category-level coverage to source-anchored capabilities, then design the raria-native API, configuration, and persistence boundaries before runtime code changes.

## Checkpoint 2: Native Architecture Boundary

Status: complete

Date: 2026-05-13

Scope completed:

- Read the current core task model, lifecycle, scheduler, registry, event bus, redb store, RPC server, RPC methods, WebSocket notification mapper, CLI command surface, daemon loop, and config types.
- Confirmed the active blockers are structural: `Gid`, `Job`, direct struct persistence, aria2 JSON-RPC, token-in-params auth, aria2 notification names, aria2-style config parsing, and parity tests are still public or near-public design anchors.
- Verified current library anchors against primary public documentation instead of memory: reqwest 0.12, suppaftp 8, russh and russh-sftp, quick-xml, librqbit, redb, axum, and RFC 5854 for Metalink v4.
- Added `native-architecture.md` with the target native task model, lifecycle, event stream, `/api/v1` HTTP JSON API, `raria.toml` layout, versioned redb schema, migration sequence, and verification requirements.

Current conclusion:

The implementation should preserve working protocol backends and execution paths, but the public and persisted model must move to native raria concepts before the project can be judged complete. The next checkpoint should add native model and schema tests, then introduce private adapters from the current `Job` model so API and persistence work can proceed without breaking downloader execution.

Next checkpoint:

Implement the first native model slice: `TaskId`, task lifecycle types, source/file/segment/piece projections, native event envelope, and schema-versioned persistence DTOs with focused unit tests.

## Checkpoint 3: Native Model Seed

Status: complete

Date: 2026-05-13

Scope completed:

- Added the first native model module in `raria-core`.
- Added `TaskId`, `TaskLifecycle`, `SourceProtocol`, `TaskSource`, `ByteRange`, `NativeEventType`, `NativeEventData`, and `NativeEvent`.
- Added schema-versioned `NativeStoreMetadata` and `NativeTaskRow` DTOs as the first persistence boundary.
- Verified the TDD loop with failing tests before implementation for both native model and native persistence DTOs.

Current conclusion:

The native model now exists, but it is intentionally not wired into the engine, API, CLI, or redb store yet. The current implementation is a small compile-safe foundation for replacing `Gid`, `Job`, aria2 JSON-RPC events, and direct struct persistence.

Next checkpoint:

Add the remaining native task projections: task summary, file entry, segment row, piece row, peer snapshot, tracker snapshot, and conversion adapters from current `Job` state for private migration use.

## Checkpoint 4: Native Projection Seed

Status: complete

Date: 2026-05-13

Scope completed:

- Added native file, segment, piece, peer, tracker, and task summary projections.
- Added a private migration projection from the current `Job` model into `NativeTaskSummary`.
- Kept the runtime path unchanged while creating a native projection layer for API, CLI, and persistence migration.
- Verified the TDD loop for projections and migration summary mapping.

Current conclusion:

raria now has a native projection vocabulary broad enough to start the `/api/v1` contract tests without exposing aria2-shaped fields. Runtime ownership still remains in the old engine model, so this is a foundation slice rather than completion of the native task model.

Next checkpoint:

Add native HTTP API contract tests for health, task listing, task creation request validation, task projection output, and event envelope serialization. Then implement the smallest axum native API module against existing engine projections.

## Checkpoint 5: Native API Seed

Status: complete

Date: 2026-05-13

Scope completed:

- Added a native `raria-rpc::api` module backed by axum.
- Added `/api/v1/health` with a native JSON response envelope.
- Added `/api/v1/tasks` returning native task projections from the existing engine through the migration adapter.
- Added contract tests proving the native endpoints do not expose JSON-RPC or `gid` fields in the tested responses.

Current conclusion:

The native HTTP API now exists as a parallel seed surface. It does not yet replace the aria2 JSON-RPC server, and it only exposes health and task listing. It is enough to begin migrating API contracts endpoint by endpoint without blocking current runtime execution.

Next checkpoint:

Extend native API contracts for task creation, task pause and resume actions, task details, files, sources, global stats, and event stream serialization.

## Checkpoint 6: Native Task Control Seed

Status: partial

Date: 2026-05-13

Scope completed:

- Added `/api/v1/tasks/{taskId}` detail endpoint.
- Added `/api/v1/tasks/{taskId}/pause` and `/api/v1/tasks/{taskId}/resume`.
- Added a native error envelope for invalid or unknown task IDs.
- Verified that tested control endpoints use native `taskId` fields and do not expose `gid`.

Current conclusion:

Native task control now covers read, pause, and resume for tasks already present in the old engine. The implementation still uses a private `task_migration_` bridge back to `Gid`, so it is a migration seed and not the final identifier architecture.

Next checkpoint:

Add native task creation, file and source subresources, global stats, and native WebSocket event serialization.

## Checkpoint 7: Native Task Creation and Subresources

Status: complete

Date: 2026-05-13

Scope completed:

- Added `POST /api/v1/tasks` with native camelCase request fields.
- Added `GET /api/v1/tasks/{taskId}/files`.
- Added `GET /api/v1/tasks/{taskId}/sources`.
- Added contract coverage for native task creation, file projection, source projection, and absence of `gid` fields in the tested native responses.

Current conclusion:

The native API can now create range-backed tasks and inspect their files and sources through raria-native JSON. This still routes through the existing engine and private migration task ID bridge, so the next structural step remains replacing the bridge with real `TaskId` ownership and native persistence indexes.

Next checkpoint:

Add global stats, native WebSocket event serialization, native remove/restart controls, and begin native `raria.toml` schema tests.

## Checkpoint 8: Native Stats and Event Serialization

Status: complete

Date: 2026-05-13

Scope completed:

- Added `GET /api/v1/stats` with native task count and speed fields.
- Added native event serialization coverage for dotted event type names such as `task.progress`.
- Adjusted native event payload serialization to camelCase field names.
- Verified that tested native stats output does not expose aria2 global stat names.

Current conclusion:

Native API coverage now includes health, stats, task creation, task listing, task detail, pause, resume, files, and sources. Native WebSocket transport is still missing, but the event envelope now has a stable JSON serialization contract.

Next checkpoint:

Implement `/api/v1/events` WebSocket transport, add native remove/restart controls, then start strict `raria.toml` schema tests.

## Checkpoint 9: Native Event WebSocket

Status: complete

Date: 2026-05-13

Scope completed:

- Added `/api/v1/events` WebSocket route to the native API server.
- Mapped current engine `DownloadEvent` values into native `NativeEvent` envelopes.
- Added contract coverage proving the event stream emits raria-native event types and does not emit JSON-RPC method frames.

Current conclusion:

The native event stream now exists. It still consumes the old engine event bus and uses migration task IDs, so it is not the final event architecture. It is now possible to migrate daemon clients from aria2-style WebSocket notifications to `/api/v1/events`.

Next checkpoint:

Add native remove/restart controls, then start strict `raria.toml` schema tests and config loading.

## Checkpoint 10: Native Remove and Restart Controls

Status: complete

Date: 2026-05-13

Scope completed:

- Added native `DELETE /api/v1/tasks/{taskId}` control.
- Added native `POST /api/v1/tasks/{taskId}/restart` control.
- Added contract coverage for remove and restart responses using native task fields.

Current conclusion:

The native task control seed now covers create, read, pause, resume, remove, and restart. Restart currently mutates the old engine registry through the migration layer, so the behavior is sufficient for API contract development but still needs to move into the native task service.

Next checkpoint:

Begin native `raria.toml` schema tests and strict config loading.

## Checkpoint 11: Native raria.toml Schema Seed

Status: complete

Date: 2026-05-13

Scope completed:

- Added `raria-core::native_config` with strict native `raria.toml` schema types.
- Added native config sections for daemon, API, downloads, network, BitTorrent, storage, and logging.
- Added tests proving native TOML loads, unknown fields fail, and legacy aria2-style names fail.

Current conclusion:

The native configuration schema now exists independently from the old aria2 key-value parser. The daemon and CLI still do not load it yet, so the next step is mapping `RariaConfig` into runtime `GlobalConfig` and replacing `--conf-path` behavior with native config loading.

Next checkpoint:

Implement native config loading from file and conversion into runtime settings used by daemon startup.

## Checkpoint 12: Native Config Runtime Bridge

Status: complete

Date: 2026-05-13

Scope completed:

- Added strict native config file loading from `raria.toml`.
- Added conversion from `RariaConfig` into the current runtime `GlobalConfig`.
- Changed CLI `--conf-path` loading to use native `raria.toml` instead of the aria2-style key-value parser.
- Updated user-facing CLI help for `--conf-path` to describe native raria TOML.

Current conclusion:

The public config-file path now points at native `raria.toml`. The old parser still exists for now because other tests and legacy internals still reference it, but it is no longer the CLI config loading path.

Next checkpoint:

Add native API authentication settings from `raria.toml`, then continue removing aria2-style public names from CLI and docs.

## Checkpoint 13: Native API Bearer Authentication

Status: complete

Date: 2026-05-13

Scope completed:

- Added optional bearer token authentication to the native API server.
- Kept `/api/v1/health` unauthenticated while protecting task, stats, and event routes when a token is configured.
- Added native API contract coverage for unauthorized and authorized requests.
- Added `raria.toml` token-file loading through `api.auth_token_file`.

Current conclusion:

The native API now has a modern bearer-token auth path and the native config schema can load the token from a file. Daemon startup still needs to pass this token into the native API server once the daemon control surface switches from JSON-RPC to native API.

Next checkpoint:

Continue removing aria2-style public names from CLI and docs, then migrate daemon startup from JSON-RPC server to native API server.

## Checkpoint 14: Daemon Native Health Endpoint

Status: partial

Date: 2026-05-13

Scope completed:

- Added a daemon smoke test for `/api/v1/health`.
- Exposed native health output on the existing daemon listener.
- Updated daemon startup text to point users at `/api/v1`.

Current conclusion:

The daemon process now exposes a native health endpoint, but the full native API router is not yet merged into the daemon listener. The old JSON-RPC listener still owns daemon routing, so this checkpoint is only the first daemon-control migration step.

Next checkpoint:

Merge the native API router into the daemon listener so task, stats, event, and auth endpoints are served by daemon mode.

## Checkpoint 15: Daemon Native API Router

Status: complete

Date: 2026-05-13

Scope completed:

- Extracted the native API router so it can run standalone or be merged into another listener.
- Merged native `/api/v1` routes into the daemon listener that currently also serves the migration JSON-RPC surface.
- Extended daemon smoke coverage from native health to native task listing.

Current conclusion:

Daemon mode now serves native API routes on the same listener as the remaining migration JSON-RPC surface. This is the first practical replacement path for daemon control clients. JSON-RPC still remains and must be removed after native endpoint coverage is broad enough.

Next checkpoint:

Add native config endpoints, then continue cutting daemon tests from JSON-RPC to `/api/v1`.

## Checkpoint 16: Native Runtime Config Endpoint

Status: complete

Date: 2026-05-13

Scope completed:

- Added native `GET /api/v1/config` runtime projection.
- Exposed daemon host, daemon port, download directory, and concurrent download limit through native field names.
- Added native API contract coverage for the config endpoint.

Current conclusion:

Native clients can now inspect the active runtime configuration through `/api/v1/config` without using the old JSON-RPC option surface. The endpoint is still backed by the current migration `GlobalConfig`, so the next step is to wire daemon startup and runtime mutation directly through native configuration state.

Validation:

- `cargo test -p raria-rpc --test native_api config_endpoint_returns_native_runtime_projection`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-core --test native_config`
- `cargo check --workspace`

Next checkpoint:

Expand daemon native API smoke coverage for task creation and controls, then continue cutting daemon tests from JSON-RPC to `/api/v1`.

## Checkpoint 17: Native Daemon Control Smoke

Status: complete

Date: 2026-05-13

Scope completed:

- Extended daemon smoke coverage to create a task through `POST /api/v1/tasks`.
- Verified daemon task pause, resume, and remove through native task-control routes.
- Added the native daemon CLI port name `--api-port` while keeping the old name as a migration alias.
- Rewrote the README public control-plane description around native raria APIs, native configuration, and the current migration status.

Current conclusion:

Daemon mode now has end-to-end native API smoke coverage for the core task lifecycle. The old JSON-RPC route and CLI alias still exist as migration scaffolding because broader session, Metalink, BitTorrent, logging, and hook regressions are still anchored there.

Validation:

- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-cli daemon_accepts_native_api_port_name`
- `cargo test -p raria-rpc --test native_api`
- `cargo check --workspace`

Next checkpoint:

Add native daemon event-stream smoke coverage, then migrate source-failure and lifecycle notification assertions from JSON-RPC WebSocket to `/api/v1/events`.

## Checkpoint 18: Native Event Smoke and Stats Auth

Status: complete

Date: 2026-05-13

Scope completed:

- Added daemon smoke coverage for `/api/v1/events`.
- Verified native daemon event frames use raria event names and omit JSON-RPC fields.
- Added a bearer-auth regression test for `/api/v1/stats`.
- Fixed `/api/v1/stats` to enforce the same bearer-token policy as the other protected native routes.

Current conclusion:

The native event stream is now covered both at the API-contract level and through the actual daemon process. Native stats no longer bypasses configured bearer authentication.

Validation:

- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-cli daemon_accepts_native_api_port_name`
- `cargo check --workspace`

Next checkpoint:

Migrate source-failure event assertions from JSON-RPC WebSocket to `/api/v1/events`, then continue replacing daemon session and protocol smoke tests with native API flows.

## Checkpoint 19: Native Source-Failure Event Smoke

Status: complete

Date: 2026-05-13

Scope completed:

- Added daemon native event smoke coverage for mirror/source failover.
- Verified `/api/v1/events` emits `task.source.failed` with native task identifiers and typed error payloads.
- Confirmed the native source-failure event frame omits JSON-RPC `method` and `jsonrpc` fields.

Current conclusion:

The highest-value source-failure notification path now has native daemon coverage. The older JSON-RPC WebSocket assertions still remain as migration regression coverage until the related daemon control and session flows are fully covered by native API tests.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_events_include_source_failover`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-rpc --test native_api`
- `cargo check --workspace`

Next checkpoint:

Begin native session persistence API coverage, then remove the matching JSON-RPC session smoke dependency once native restore and save flows have equivalent tests.

## Checkpoint 20: Native Session Save API

Status: complete

Date: 2026-05-13

Scope completed:

- Added native `POST /api/v1/session/save`.
- Returned native save status, persisted task count, and session path.
- Added API contract coverage for session save.
- Added daemon smoke coverage proving a real daemon process can persist the session through the native API.
- Added bearer-auth coverage for the native session save route.

Current conclusion:

Manual session persistence no longer requires the old JSON-RPC `saveSession` surface. Restore coverage is still mostly anchored by existing daemon session smoke tests, so the next step is native restart/restore verification using `/api/v1/tasks`.

Validation:

- `cargo test -p raria-rpc --test native_api session_save_endpoint_reports_native_store_status`
- `cargo test -p raria-rpc --test native_api native_api_uses_bearer_token_auth_when_configured`
- `cargo test -p raria-cli --test native_api_smoke daemon_exposes_native_api_endpoints`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo check --workspace`

Next checkpoint:

Add native daemon restart/restore smoke coverage through `/api/v1/tasks`, then retire the matching JSON-RPC-only restore assertion.

## Checkpoint 21: Native Restore Smoke

Status: complete

Date: 2026-05-13

Scope completed:

- Added daemon restart/restore smoke coverage using only native API routes.
- Verified a task saved by `POST /api/v1/session/save` is visible after daemon restart through `GET /api/v1/tasks`.
- Verified restored task projections use native `taskId` fields and omit `gid`.

Current conclusion:

The core session save and restore loop now has native daemon coverage. The store schema is still the migration-era redb table layout, so this does not complete native persistence. It gives the migration a native regression anchor before versioned redb schemas are introduced.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_restores_saved_task_through_native_api`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-rpc --test native_api`
- `cargo check --workspace`

Next checkpoint:

Start versioned native redb schema work with metadata and task-row tables, then migrate restore/save internals away from direct `Job` serialization.

## Checkpoint 22: Native Store Schema Seed

Status: complete

Date: 2026-05-13

Scope completed:

- Added native redb metadata table initialization.
- Added versioned native task-row table operations.
- Added native task-row get, put, and list tests.
- Kept the existing migration `jobs` and segment tables intact while introducing the native schema seed.

Current conclusion:

The store now has a versioned native schema entry point. Runtime save and restore still rely on direct `Job` serialization, so native task rows are not yet the authoritative persistence path.

Validation:

- `cargo test -p raria-core persist::tests::native_metadata_is_created_when_store_opens`
- `cargo test -p raria-core persist::tests::native_task_rows_roundtrip_by_task_id`
- `cargo test -p raria-core persist::tests::list_native_task_rows_returns_all_rows`
- `cargo test -p raria-core persist::tests`
- `cargo test -p raria-core --test native_config`
- `cargo check --workspace`

Next checkpoint:

Persist native task rows during session save, then start replacing restore internals with native row loading.

## Checkpoint 23: Native Task Rows on Session Save

Status: complete

Date: 2026-05-13

Scope completed:

- Updated session save to persist versioned native task rows alongside the migration `Job` rows.
- Added engine coverage proving queued and paused lifecycle states are written into native task rows.
- Preserved the current restore behavior while seeding native persistence state for the next migration step.

Current conclusion:

Native task rows now receive real runtime data during session save. They are still a parallel persistence path, not yet the restore source of truth.

Validation:

- `cargo test -p raria-core engine::tests::save_session_persists_native_task_rows`
- `cargo test -p raria-core persist::tests`
- `cargo test -p raria-core engine::tests::save_session_persists`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo check --workspace`

Next checkpoint:

Add native task-row lifecycle migration tests for restore, then migrate restore internals away from direct `Job` rows without losing existing session behavior.

## Checkpoint 24: Native Task Row Restore Path

Status: complete

Date: 2026-05-13

Scope completed:

- Extended native task rows with source URIs, output path, byte progress, total bytes, and segment count.
- Added conversion coverage from migration `Job` state into native task rows.
- Added conversion coverage from native task rows back into migration `Job` state.
- Changed engine restore to prefer native task rows when present, with old `Job` rows retained as migration fallback.
- Preserved existing restore semantics for queued, active, seeding, completed, and paused jobs.

Current conclusion:

Restore now has a native-row source of truth when native rows exist. The conversion still maps back into the migration `Job` runtime model because native `TaskId` ownership has not replaced the engine registry yet.

Validation:

- `cargo test -p raria-core native_persist_tests::task_row_carries_migration_job_restore_fields`
- `cargo test -p raria-core native_persist_tests::task_row_restores_migration_job_fields`
- `cargo test -p raria-core engine::tests::engine_restore_prefers_native_task_rows_when_available`
- `cargo test -p raria-core engine::tests::engine_restore`
- `cargo test -p raria-core native_persist_tests`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-rpc --test native_api`
- `cargo check --workspace`

Next checkpoint:

Introduce a native task index owned by `TaskId`, then start routing native API lookups through that index instead of parsing migration GIDs from task IDs.

## Checkpoint 25: Native Task Index Lookup

Status: complete

Date: 2026-05-14

Scope completed:

- Added an in-memory native task index that maps `TaskId` values to current runtime job ids.
- Registered migration tasks in the index during task submission and restore.
- Added engine tests for native id registration, lookup, and restore registration.
- Added a native API contract test proving task lookup can resolve an index-owned native task id instead of parsing a migration id string.
- Updated native API task lookup to use the engine index instead of decoding migration ids inside the API layer.

Current conclusion:

Native API task lookup no longer owns migration id parsing. The engine still maps native task ids onto the current `Gid` runtime model, so full native task ownership remains incomplete.

Validation:

- `cargo test -p raria-core native_projection_tests::native_task_index`
- `cargo test -p raria-core engine::tests::register_native_task_id_for_migration_requires_existing_job`
- `cargo test -p raria-core engine::tests::engine_restore`
- `cargo test -p raria-rpc --test native_api task_detail_resolves_native_task_index_ids`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo check --workspace`

Next checkpoint:

Make native API response projections use the engine task index consistently, then begin moving task creation toward non-migration `TaskId` ownership.

## Checkpoint 26: Indexed Native Task Projections

Status: complete

Date: 2026-05-14

Scope completed:

- Updated native task summary projection inside the API layer to use the engine task index.
- Added contract coverage proving `GET /api/v1/tasks` projects index-owned native task ids.
- Tightened detail coverage so `GET /api/v1/tasks/{taskId}` returns the indexed native task id instead of a migration-derived id.
- Kept the migration `Gid` runtime bridge internal while improving public projection behavior.

Current conclusion:

Native API lookup and response projection now consistently use the engine task index. New task creation still registers deterministic migration task ids, so true non-migration task id ownership is still pending.

Validation:

- `cargo test -p raria-rpc --test native_api task_detail_resolves_native_task_index_ids`
- `cargo test -p raria-rpc --test native_api tasks_endpoint_projects_native_task_index_ids`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-core native_projection_tests::native_task_index`
- `cargo test -p raria-core engine::tests::register_native_task_id_for_migration_requires_existing_job`
- `cargo check --workspace`

Next checkpoint:

Generate non-migration `TaskId` values for native API task creation while keeping the current runtime `Gid` bridge internal.

## Checkpoint 27: Opaque Native Task Creation

Status: complete

Date: 2026-05-14

Scope completed:

- Changed native API task creation to generate opaque `TaskId` values instead of returning `task_migration_*` ids.
- Updated daemon smoke coverage to reject migration task ids for native task creation.
- Updated native event conversion to project indexed native task ids.
- Preserved opaque task ids through native session save and daemon restart/restore.
- Added a temporary `runtime_bridge_id` field to native task rows so opaque task ids can restore into the current migration runtime until engine ownership moves fully to `TaskId`.

Current conclusion:

Native API task creation, control, event projection, session save, and restart/restore now preserve opaque native task ids at the public surface. Internally, the runtime still bridges to numeric `Gid` values.

Validation:

- `cargo test -p raria-rpc --test native_api task_creation_files_and_sources_are_native_resources`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-core native_persist_tests`
- `cargo test -p raria-core engine::tests::save_session`
- `cargo test -p raria-core engine::tests::engine_restore`
- `cargo check --workspace`

Next checkpoint:

Continue removing migration identifier assumptions from native API events and persistence fixtures, then move runtime registry ownership toward `TaskId`.

## Checkpoint 66: Daemon Native Transfer and Mutation Smoke

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon smoke coverage for native global transfer policy mutation through `PATCH /api/v1/transfer`.
- Added daemon smoke coverage for waiting-task transfer policy mutation through `PATCH /api/v1/tasks/{taskId}/transfer`.
- Added daemon smoke coverage for waiting-task range source replacement through `PATCH /api/v1/tasks/{taskId}/sources`.
- Added daemon smoke coverage for waiting-task queue reordering through `PATCH /api/v1/tasks/{taskId}/queue`.
- Verified the tested daemon responses use raria-native camelCase fields and do not expose aria2 option names or JSON-RPC envelopes.

Current conclusion:

Native API mutation routes are now covered at both contract-test and running-daemon smoke levels for global transfer policy, per-task transfer policy, range source replacement, and queue position changes. This improves confidence that `/api/v1` can replace JSON-RPC for these runtime controls. The remaining public-surface work is still substantial: JSON-RPC remains present, BT runtime mutation smoke is incomplete, and the native CLI still needs to move away from aria2-shaped options.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_transfer_policy_mutates_runtime_state -- --nocapture`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_task_mutation_routes_update_waiting_tasks -- --nocapture`

Next checkpoint:

Migrate another JSON-RPC-dependent protocol/control smoke area to native `/api/v1`, prioritizing BitTorrent runtime state and event coverage.

## Checkpoint 67: Native BitTorrent Daemon Metadata and Peer Smoke

Status: complete

Date: 2026-05-16

Scope completed:

- Added a native daemon BitTorrent smoke test that creates a `torrent:base64` task through `POST /api/v1/tasks`.
- Reused a local librqbit seeder and local HTTP tracker fixture to avoid external network dependency.
- Verified `/api/v1/events` emits `task.bt.metadata.resolved` for the native task id.
- Verified `/api/v1/tasks/{taskId}/trackers` exposes the configured native tracker URI after a real tracker announce.
- Verified `/api/v1/tasks/{taskId}/peers` exposes live native peer fields without aria2 peer response fields.

Current conclusion:

The running daemon now has native smoke coverage for the core BitTorrent visibility path: native task creation, metadata eventing, tracker projection, peer projection, and tracker announce behavior. This does not complete BitTorrent modernization. Remaining gaps include native torrent-file smoke coverage, live tracker replacement limits, UDP tracker behavior, PEX, WebSeed lifecycle coverage, seed-only/stop-timeout behavior, and fastresume binding to versioned raria persistence.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_exposes_live_bt_metadata_peers_and_trackers -- --nocapture`

Next checkpoint:

Continue replacing JSON-RPC-dependent BitTorrent smoke coverage with native `/api/v1`, focusing on file selection, seeding lifecycle, WebSeed, and native persistence.

## Checkpoint 68: Native BitTorrent File Selection Daemon Smoke

Status: complete

Date: 2026-05-16

Scope completed:

- Added a native daemon smoke test for multi-file BitTorrent file selection.
- Created a local multi-file torrent fixture and submitted it through `POST /api/v1/tasks` with `bt.selectedFileIds`.
- Verified `/api/v1/tasks/{taskId}/files` exposes native file ids and selected states without aria2 file indexes.
- Verified `PATCH /api/v1/tasks/{taskId}/files` updates selected file ids through native field names.
- Verified the selection state remains visible after the BT runtime sync loop, which anchors the route to librqbit `only_files` state instead of only testing registry mutation.

Current conclusion:

Native file selection now has running-daemon coverage for initial selection and live mutation. This narrows another JSON-RPC dependency. Remaining BitTorrent gaps still include native seeding lifecycle smoke, WebSeed lifecycle visibility, torrent-file source smoke, UDP tracker behavior, PEX, live tracker replacement limits, selected-file cleanup behavior, and fastresume binding to versioned raria persistence.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_updates_live_bt_file_selection -- --nocapture`

Next checkpoint:

Continue native BitTorrent smoke migration with seeding lifecycle or WebSeed visibility, then remove corresponding JSON-RPC-only assumptions.

## Checkpoint 69: Native BitTorrent WebSeed Seeding Lifecycle Smoke

Status: complete

Date: 2026-05-16

Scope completed:

- Added a deterministic daemon smoke test for native BitTorrent seeding lifecycle after WebSeed-backed completion.
- Created a local single-file torrent fixture and a local HTTP WebSeed server with byte-range support.
- Submitted the torrent through native `POST /api/v1/tasks` using `bt.webSeedUris` and `bt.seeding`.
- Verified `/api/v1/tasks/{taskId}/bt/seeding` exposes native `targetRatio` and `stopAfterMinutes` fields without aria2 option names.
- Verified `/api/v1/events` emits `task.bt.seeding.started` with native event data and no JSON-RPC notification fields.
- Verified `/api/v1/tasks/{taskId}` reaches `lifecycle = seeding` with completed byte projection matching the WebSeed payload.
- Replaced the flaky peer-transfer-dependent seeding smoke with a WebSeed-backed fixture so CI does not depend on a real BT peer completing a transfer.

Current conclusion:

Native BitTorrent seeding lifecycle now has running-daemon coverage through the raria-native API and event stream. WebSeed is also anchored at daemon level for single-file torrent completion. Remaining BitTorrent gaps still include native torrent-file path smoke, UDP tracker behavior, PEX, richer peer state, live tracker replacement limits, stop-timeout and seed-only lifecycle coverage, selected-file cleanup behavior, and fastresume binding to versioned raria persistence.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_emits_bt_seeding_lifecycle -- --nocapture`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_api`

Next checkpoint:

Continue native BitTorrent smoke migration with torrent-file source coverage or stop-timeout lifecycle coverage, then remove corresponding JSON-RPC-only assumptions.

## Checkpoint 70: Native BitTorrent Torrent File Source Smoke

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon smoke coverage for submitting a local `.torrent` file path through native `POST /api/v1/tasks`.
- Extended native source detection so `.torrent` file paths project as `SourceProtocol::Torrent`.
- Routed `.torrent` path tasks into the BitTorrent runtime instead of the range-download executor.
- Reused the deterministic local WebSeed fixture to prove torrent-file metadata resolution and seeding lifecycle without relying on an external peer.
- Verified the created native task exposes `protocol = torrent`, keeps the original torrent-file path as the source URI, emits `task.bt.metadata.resolved`, reaches `lifecycle = seeding`, and does not expose legacy `gid` fields.

Current conclusion:

Native BitTorrent torrent-file ingestion now has daemon-level raria-native API coverage for local `.torrent` paths. Remaining BitTorrent gaps include UDP tracker behavior, PEX, richer peer state, live tracker replacement limits, stop-timeout and seed-only lifecycle coverage, selected-file cleanup behavior, and fastresume binding to versioned raria persistence.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_accepts_torrent_file_sources -- --nocapture`

Next checkpoint:

Continue native BitTorrent lifecycle migration with stop-timeout or seed-only coverage, then remove corresponding JSON-RPC-only assumptions.

## Checkpoint 71: Native BitTorrent Idle Download Timeout Policy

Status: complete

Date: 2026-05-16

Scope completed:

- Audited aria2's `bt-stop-timeout` behavior from the manual and source, confirming it stops incomplete BitTorrent downloads after consecutive zero download speed rather than after seeding begins.
- Added the raria-native policy field `idleDownloadTimeoutSeconds` under the native BitTorrent seeding policy resource instead of exposing aria2 option names.
- Stored the policy in `JobOptions::bt_idle_download_timeout` and wired `POST /api/v1/tasks` plus `PATCH /api/v1/tasks/{taskId}/bt/seeding` through the native task facade.
- Updated daemon BitTorrent runtime logic to fail an incomplete torrent after the configured zero-speed idle window and reset the idle window when download speed recovers.
- Extended daemon native smoke coverage to verify the field round-trips without legacy `bt-stop-timeout` names.

Current conclusion:

Native BitTorrent idle download timeout is now modeled and enforced as a raria-native policy. Remaining BitTorrent lifecycle gaps include seed-only lifecycle behavior, seed-only detachment from active-task concurrency, UDP tracker behavior, PEX, richer peer state, live tracker replacement limits, selected-file cleanup behavior, and fastresume binding to versioned raria persistence.

Validation:

- `cargo test -p raria-rpc --test native_api task_bt_seeding_patch_updates_native_seed_policy`
- `cargo test -p raria-cli bt_runtime::tests::bt_idle_timeout`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_emits_bt_seeding_lifecycle -- --nocapture`

Next checkpoint:

Continue native BitTorrent lifecycle migration with seed-only lifecycle and concurrency-detachment coverage, then remove corresponding JSON-RPC-only assumptions.

## Checkpoint 72: Native BitTorrent Seed-Only Scheduling Detachment

Status: complete

Date: 2026-05-16

Scope completed:

- Audited aria2's `bt-detach-seed-only` behavior from the manual and source, confirming the modern behavior is to keep seeding tasks visible as active lifecycle tasks while excluding them from download concurrency.
- Added scheduler coverage proving native task activation no longer counts `Status::Seeding` tasks against the bounded active download slots.
- Changed native and migration scheduler activation calculations to count only actively downloading tasks for queue admission.
- Added runtime coverage proving a BitTorrent transition into seeding wakes the daemon work loop so a newly freed download slot can activate queued tasks promptly.
- Updated native BitTorrent seeding event publication to notify the scheduler after seeding starts.

Current conclusion:

Native scheduling now treats seeding as a lifecycle state that remains externally active but does not consume download concurrency. Remaining BitTorrent lifecycle work includes daemon-level seed-only queue activation smoke, UDP tracker behavior, PEX, richer peer state, live tracker replacement limits, selected-file cleanup behavior, and fastresume binding to versioned raria persistence.

Validation:

- `cargo test -p raria-core scheduler::tests::native_tasks_to_activate_does_not_count_seeding_tasks_as_download_slots`
- `cargo test -p raria-core scheduler::tests::jobs_to_activate`
- `cargo test -p raria-cli bt_runtime::tests::sync_bt_job_from_status_notifies_scheduler_when_entering_seeding`

Next checkpoint:

Add daemon-level seed-only queue activation smoke with native task ids, then continue replacing JSON-RPC-only BitTorrent lifecycle assumptions.

## Checkpoint 73: Native BitTorrent Seed-Only Daemon Queue Smoke

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon native smoke coverage for seed-only queue activation with `--max-concurrent 1`.
- Reused the deterministic local WebSeed torrent fixture so the first BitTorrent task reaches `lifecycle = seeding` without external peers.
- Submitted a waiting range task behind the BitTorrent task through native `POST /api/v1/tasks`.
- Verified the waiting task leaves `queued` after the BitTorrent task enters seed-only lifecycle, proving daemon scheduling observes seed-only detachment on the real process path.
- Verified the activated waiting task response remains raria-native and does not expose legacy `gid` fields.

Current conclusion:

Seed-only queue activation is now covered at the daemon API level. Remaining BitTorrent lifecycle and storage work includes selected-file cleanup behavior, fastresume binding to versioned raria persistence, UDP tracker behavior, PEX, richer peer state, and live tracker replacement limits.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_bt_seeding_frees_download_concurrency_for_waiting_tasks -- --nocapture`

Next checkpoint:

Continue BitTorrent completion work with selected-file cleanup or fastresume binding to versioned raria persistence, then reassess remaining JSON-RPC-only BitTorrent assumptions.

## Checkpoint 74: Native BitTorrent Selected-File Cleanup

Status: complete

Date: 2026-05-16

Scope completed:

- Audited aria2's `bt-remove-unselected-file` behavior from the manual and source, confirming the modern capability is explicit deletion of unselected BitTorrent files after selected payload completion.
- Added raria-native task creation field `bt.deleteUnselectedFilesOnCompletion` without reusing aria2 option names.
- Persisted the policy in `JobOptions` and added a native engine mutation path behind `TaskId`.
- Added focused runtime coverage proving only unselected torrent files are removed.
- Added daemon native smoke coverage with a multi-file WebSeed torrent, selected-file completion, selected file preservation, and unselected file deletion.

Current conclusion:

Selected-file cleanup is now implemented through the raria-native creation surface and verified on the real daemon path. Remaining BitTorrent storage and network gaps include fastresume binding to versioned raria persistence, UDP tracker behavior, PEX, richer peer state, and live tracker replacement limits.

Validation:

- `cargo test -p raria-rpc --test native_api task_creation_accepts_native_bt_options`
- `cargo test -p raria-cli bt_runtime::tests::cleanup_unselected_bt_files_removes_only_unselected_paths`
- `cargo test -p raria-cli --test native_api_smoke daemon_native_bt_deletes_unselected_files_after_completion -- --nocapture`

Next checkpoint:

Tie BitTorrent fastresume state to versioned raria persistence or verify current librqbit persistence has enough stable hooks for a native binding layer.

## Checkpoint 75: Native BitTorrent Fastresume State Directory Binding

Status: complete

Date: 2026-05-16

Scope completed:

- Audited aria2 resume/control-file behavior and raria's current librqbit fastresume path.
- Verified current librqbit public APIs expose `fastresume` and `SessionPersistenceConfig::Json { folder }`, but not a stable API for embedding fastresume blobs directly into raria's redb store.
- Added a raria-bt configuration hook for an explicit session persistence directory while preserving the download-dir scoped default for direct `BtService` users.
- Bound daemon-created BT services to a stable native path derived from `GlobalConfig.session_file`: `<session-file>.bt-session`.
- Added focused raria-bt contract coverage for the native persistence directory.
- Added daemon smoke coverage proving real BT fastresume state is written under the session-derived native directory and not the old download-scoped default.

Current conclusion:

BitTorrent fastresume state is now attached to the raria session location instead of drifting with the download directory. Because librqbit only exposes filesystem JSON persistence today, the next native persistence step is to add an explicit versioned raria metadata row that records the external BT state directory and validates it during restore.

Validation:

- `cargo test -p raria-bt --test dht_persistence bt_session_persistence_contract_accepts_native_raria_state_dir`
- `cargo test -p raria-cli bt_runtime::tests::bt_service_config_binds_fastresume_to_native_session_path`
- `cargo test -p raria-cli --test bt_tracker_smoke daemon_binds_bt_fastresume_state_to_native_session_path -- --nocapture`

Next checkpoint:

Add versioned native metadata for external BitTorrent runtime state and daemon restore validation for the session-derived BT fastresume directory.

## Checkpoint 76: Native BitTorrent UDP Tracker Runtime Anchor

Status: complete

Date: 2026-05-16

Scope completed:

- Audited aria2's UDP tracker path from the manual and BitTorrent source references, then treated UDP tracker announce as a modern in-scope tracker capability.
- Verified current librqbit public documentation exposes add-time tracker URI forwarding through `AddTorrentOptions.trackers`, while no separate raria-native UDP tracker layer is needed for the basic announce path.
- Added a local UDP tracker fixture that implements the BitTorrent UDP tracker connect and announce responses.
- Added raria-bt runtime smoke coverage proving a torrent with only a `udp://` tracker discovers a local seed peer, completes a real download, and keeps the UDP announce URI visible in status.

Current conclusion:

The basic UDP tracker announce path is now anchored by a real local transfer through the mature BitTorrent backend. The remaining tracker work is daemon-level native API projection for UDP tracker task creation plus richer timeout, interval, exclusion, and live replacement behavior.

Validation:

- `cargo test -p raria-bt --test bt_smoke bt_service_downloads_real_torrent_through_udp_tracker -- --nocapture`

Next checkpoint:

Add daemon-level native API smoke coverage for UDP tracker task creation and tracker projection, then continue the PEX evidence audit.

## Checkpoint 77: Native BitTorrent UDP Tracker Daemon Projection

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon-level native API smoke coverage for `bt.trackerUris` containing a `udp://` tracker.
- Reused the local UDP tracker fixture and added announce counting so the smoke verifies real UDP announce traffic instead of relying on a transient peer-list snapshot.
- Verified native task creation, native tracker projection, completed transfer state, and absence of aria2-shaped response fields on the UDP tracker path.

Current conclusion:

UDP tracker support is now complete for the modern target's basic announce and projection path. Remaining tracker-management work is broader policy work: timeout, interval, exclusion controls, and live replacement behavior.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_exposes_udp_bt_tracker_projection -- --nocapture`

Next checkpoint:

Continue the PEX evidence audit against aria2 source and librqbit public APIs, then either implement a mature-backed PEX path or document a proven library limitation with an executable contract.

## Checkpoint 78: Native BitTorrent PEX Capability Anchor

Status: complete

Date: 2026-05-16

Scope completed:

- Audited aria2's PEX behavior from the manual, `UTPexExtensionMessage`, `DefaultBtInteractive`, and peer interaction wiring.
- Confirmed aria2's modern behavior is BEP-10 `ut_pex`, default enabled, disabled for private torrents, and periodically exchanged through compact added/dropped peer lists.
- Added a local peer handshake probe that performs a real BitTorrent handshake with librqbit through `BtService` and captures the extended handshake.
- Verified the mature backend advertises `ut_pex` through `bt_service_advertises_ut_pex_in_extended_handshake`.
- Added native config plumbing so `bittorrent.enable_pex` reaches `GlobalConfig` and `BtServiceConfig` instead of remaining a parsed-only field.
- Added a limitation contract proving current librqbit public APIs still advertise `ut_pex` when raria's native PEX policy is disabled.

Current conclusion:

PEX is now anchored as a mature-backend capability rather than an unverified gap. raria can carry a native PEX policy, and the active backend advertises `ut_pex`. A stable disable hook is not available through the current exercised public librqbit path, so policy enforcement remains partial until upstream support is proven or a small native peer-protocol gate is justified.

Validation:

- `cargo test -p raria-bt --test bt_smoke bt_service_advertises_ut_pex_in_extended_handshake -- --nocapture`
- `cargo test -p raria-bt --test bt_smoke bt_service_pex_disable_policy_is_not_enforced_by_backend_public_api -- --nocapture`
- `cargo test -p raria-cli bt_runtime::tests::bt_service_config_forwards_native_pex_policy`
- `cargo test -p raria-core native_config_carries_pex_policy_into_runtime_config`

Next checkpoint:

Move to richer peer state or tracker-management policy. Revisit PEX only if upstream exposes a stable disable hook or if native protocol gating becomes necessary.

## Checkpoint 79: Native BitTorrent Tracker Policy Model

Status: complete

Date: 2026-05-16

Scope completed:

- Audited aria2 tracker controls covering additional trackers, excluded trackers, tracker connect timeout, tracker request timeout, and tracker interval.
- Extended native tracker snapshots with `excluded`, `connectTimeoutSeconds`, `timeoutSeconds`, and `intervalSeconds`.
- Added native task option storage for excluded tracker URIs and tracker timing policy.
- Extended `PATCH /api/v1/tasks/{taskId}/trackers` to accept native tracker policy fields alongside tracker URI replacement.
- Added contract coverage proving the native route persists and projects tracker exclusion and timing policy without aria2 option names.

Current conclusion:

Tracker policy now has a native API and task-model shape. Runtime enforcement remains partial because the current mature backend path already supports add-time tracker URIs but has not yet exposed stable live hooks for enforcing exclusion, timeout, interval, or replacement after submission.

Validation:

- `cargo test -p raria-rpc --test native_api task_trackers_patch_updates_native_bt_trackers`

Next checkpoint:

Verify mature backend hooks for tracker timeout, interval, exclusion, and live replacement. If the hooks remain absent, document the limitation and continue with richer peer state or native source graph work.

## Checkpoint 50: Native Event Projection Uses Registered Task IDs

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API event-stream regression coverage proving unmapped legacy `Gid` events are not projected as synthetic `task_migration_*` events.
- Changed native event projection to require an engine-registered native `TaskId` for task-scoped events.
- Preserved projection of mapped progress events with the opaque native task id returned by native task creation.

Current conclusion:

The native `/api/v1/events` stream no longer invents migration task ids for unknown legacy event-bus messages. It now treats the engine's native task-id mapping as the authority while the old `DownloadEvent` bus remains a temporary transport.

Validation:

- `cargo test -p raria-rpc --test native_api native_events_websocket_streams_raria_event_envelopes`

Next checkpoint:

Move the remaining range progress and source-failure publication paths toward native event payloads so `DownloadEvent` can shrink to the legacy JSON-RPC notification surface.

## Checkpoint 51: Native Event Bus Foundation

Status: complete

Date: 2026-05-16

Scope completed:

- Added a `NativeEventBus` alongside the legacy `DownloadEvent` bus.
- Updated `update_native_progress` to publish native `task.progress` events with native task ids and typed progress payloads.
- Updated `source_failed_native_task` to publish native `task.source.failed` events while preserving the legacy notification bus for JSON-RPC migration coverage.
- Updated `/api/v1/events` to consume the native event bus first and fall back to mapped legacy events during the transition.
- Added core coverage for native progress and source-failure event publication.
- Added native API coverage proving the WebSocket stream prefers native events over unmapped legacy bus messages.

Current conclusion:

Native progress and source-failure paths now have a native event transport. The legacy event bus still exists for the aria2-shaped JSON-RPC notification surface and as a fallback projection while remaining lifecycle events are migrated.

Validation:

- `cargo test -p raria-core engine::tests::native_runtime_helpers_publish_native_progress_and_source_failure_events`
- `cargo test -p raria-rpc --test native_api native_events_websocket_prefers_native_event_bus`

Next checkpoint:

Move native lifecycle events for start, pause, resume, completion, failure, and removal onto the native event bus, then reduce `/api/v1/events` dependence on legacy event projection.

## Checkpoint 52: Native Lifecycle Events

Status: complete

Date: 2026-05-16

Scope completed:

- Added a shared native lifecycle event publication helper in the engine.
- Published native `task.created`, `task.started`, `task.paused`, `task.resumed`, `task.completed`, `task.failed`, and `task.removed` events from task lifecycle paths.
- Kept legacy `DownloadEvent` publication for JSON-RPC notification compatibility during migration.
- Updated `/api/v1/events` to suppress legacy fallback projection after native events are observed on a connection, preventing duplicate lifecycle frames.
- Added core coverage for native lifecycle event publication.
- Added native API WebSocket coverage for native lifecycle event delivery.

Current conclusion:

Core task lifecycle changes now reach the native event stream directly. The old event bus remains only for legacy JSON-RPC notifications and as a temporary fallback for event types that have not yet been moved.

Validation:

- `cargo test -p raria-core engine::tests::native_lifecycle_operations_publish_native_events`
- `cargo test -p raria-rpc --test native_api native_events_websocket_streams_native_lifecycle_events`

Next checkpoint:

Move the remaining protocol-specific lifecycle and BitTorrent events onto native event types, then continue removing `/api/v1/events` fallback projection.

## Checkpoint 53: Native BitTorrent Metadata Event Foundation

Status: complete

Date: 2026-05-16

Scope completed:

- Added native event types for `task.bt.metadata.resolved`, `task.bt.seeding.started`, `task.bt.peer.updated`, and `task.bt.tracker.updated`.
- Added typed native BitTorrent event payloads for metadata, seeding, peer, and tracker updates.
- Added serialization coverage proving native BT event type strings and payload field names are stable.
- Added an engine helper for publishing native BitTorrent metadata resolution events.
- Updated BT status synchronization to publish `task.bt.metadata.resolved` once librqbit status exposes metadata.
- Added BT runtime coverage proving metadata sync publishes the native event with the task id and metadata payload.

Current conclusion:

The native event schema now has first-class BitTorrent metadata, seeding, peer, and tracker event types. Metadata resolution is wired to the current BT status sync path; seeding, peer, and tracker event types are available for the next runtime wiring slice.

Validation:

- `cargo test -p raria-core native_projection_tests::bt_native_events_use_stable_type_strings_and_payloads`
- `cargo test -p raria-cli bt_runtime::tests::sync_bt_job_from_status_populates_bt_snapshot_fields`

Next checkpoint:

Wire native BT seeding, peer, and tracker update events from the daemon runtime, then expose native BT peer and tracker resources through `/api/v1`.

## Checkpoint 54: Native BitTorrent Peer and Tracker Events and API

Status: complete

Date: 2026-05-16

Scope completed:

- Added native task peer and tracker projections in the engine.
- Added `/api/v1/tasks/{taskId}/peers` and `/api/v1/tasks/{taskId}/trackers`.
- Added native API contract coverage proving peer and tracker resources use raria-native field names and do not expose aria2 peer fields.
- Added engine helpers for publishing native BT seeding, peer, and tracker update events.
- Updated BT status synchronization to publish `task.bt.seeding.started`, `task.bt.peer.updated`, and `task.bt.tracker.updated` events.
- Added BT runtime coverage for seeding, peer, and tracker native event publication.

Current conclusion:

Native API clients can now read BT peer and tracker snapshots without using JSON-RPC. BT status sync also emits native seeding, peer, and tracker event frames, closing the primary BT event-schema gap left after metadata resolution.

Validation:

- `cargo test -p raria-rpc --test native_api task_peers_and_trackers_are_native_resources`
- `cargo test -p raria-cli bt_runtime::tests::sync_bt_job_from_status`

Next checkpoint:

Add daemon smoke coverage for native BT peer/tracker resources and continue moving tracker mutation and selected-file controls from aria2-shaped RPC methods to native `/api/v1`.

## Checkpoint 55: Native BitTorrent File Selection API

Status: complete

Date: 2026-05-16

Scope completed:

- Updated native task summaries to project BitTorrent file snapshots instead of collapsing BT tasks to a single output file.
- Added coverage proving native BT file projections expose native `file_N` ids, paths, progress, and selection state.
- Added native task file-selection mutation in the engine using stable native file ids.
- Added `PATCH /api/v1/tasks/{taskId}/files` for updating selected BT files.
- Added native API contract coverage proving file selection updates do not expose aria2 file index fields and update runtime selected-file options.

Current conclusion:

Native clients can now inspect BT file snapshots and update selected files through `/api/v1` without using the aria2-shaped `select-file` RPC option. Live backend mutation still needs runtime wiring once the BT task has already been submitted to librqbit.

Validation:

- `cargo test -p raria-core native_projection_tests::task_summary_projection_uses_bt_file_snapshots`
- `cargo test -p raria-rpc --test native_api task_files_patch_updates_native_bt_file_selection`

Next checkpoint:

Add native tracker mutation endpoints and live BT selected-file/tracker runtime wiring, then expand daemon smoke coverage for BT native resources.

## Checkpoint 56: Native BitTorrent Tracker Mutation API

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage for replacing BitTorrent tracker URIs through `PATCH /api/v1/tasks/{taskId}/trackers`.
- Added engine-level native tracker mutation using `TaskId` and raria-native request fields.
- Updated tracker mutation to refresh both runtime tracker snapshots and stored tracker options.
- Kept the response as `NativeTrackerSnapshot` resources and verified the native response does not expose aria2 `bt-tracker` fields.

Current conclusion:

Native clients can now read and replace BT tracker URIs through `/api/v1` without using the aria2-shaped `bt-tracker` RPC option. Live submitted librqbit tracker mutation is still a follow-up runtime wiring gap, and tracker timeout, interval, and exclusion controls still need native resource design.

Validation:

- `cargo test -p raria-rpc --test native_api task_trackers_patch_updates_native_bt_trackers`

Next checkpoint:

Wire native BT file-selection and tracker mutations into the live librqbit runtime where the library permits it, then add daemon smoke coverage for native BT resource updates.

## Checkpoint 57: Live BitTorrent File Selection Runtime Wiring

Status: complete

Date: 2026-05-16

Scope completed:

- Verified current librqbit 8.1.1 exposes live file-selection mutation through `Session::update_only_files` and `Api::api_torrent_action_update_only_files`.
- Added `BtService::update_selected_files` as the raria-native service wrapper around librqbit live file selection.
- Updated BT daemon polling to detect native selected-file option changes and forward changed selections to the active librqbit torrent handle.
- Added focused runtime helper coverage proving selected-file comparison is order-insensitive and only requests live updates when the selected set changes.
- Recorded that current librqbit public APIs expose add-time trackers but do not expose an equivalent live tracker mutation API.

Current conclusion:

Native BT file-selection changes made through `/api/v1/tasks/{taskId}/files` now reach active librqbit downloads on the next BT runtime poll. Live tracker mutation remains incomplete because the current mature library surface does not expose a direct live tracker replacement hook; raria still updates stored tracker options and snapshots, and a focused native layer may be required if upstream support remains absent.

Validation:

- `cargo test -p raria-cli bt_runtime::tests::selected_files_changed_uses_set_semantics_for_live_bt_updates`

Next checkpoint:

Add daemon smoke coverage for native BT resource updates, then continue replacing JSON-RPC BT control tests with `/api/v1` contracts.

## Checkpoint 58: Native BitTorrent Seeding Policy API

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage for reading and updating BitTorrent seeding policy through `/api/v1/tasks/{taskId}/bt/seeding`.
- Added native request and response fields `targetRatio` and `stopAfterMinutes`.
- Added engine-level native seeding policy mutation through `TaskId`.
- Verified the native response does not expose aria2 `seed-ratio` option fields while still updating the runtime seeding policy consumed by the BT daemon.

Current conclusion:

Native clients can now control BT ratio/time seeding policy without using aria2-shaped `changeOption` or `getOption`. Stop timeout and seed-only lifecycle controls still need native endpoint coverage.

Validation:

- `cargo test -p raria-rpc --test native_api task_bt_seeding_patch_updates_native_seed_policy`

Next checkpoint:

Continue replacing JSON-RPC BT control tests with native API contracts, focusing on add-time torrent options and native WebSeed task creation.

## Checkpoint 59: Native BitTorrent Creation Options

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage for BitTorrent add-time options on `POST /api/v1/tasks`.
- Added native creation fields under `bt.selectedFileIds`, `bt.trackerUris`, `bt.webSeedUris`, and `bt.seeding`.
- Reused the native task facade after task creation so add-time options are stored through `TaskId` paths instead of aria2-shaped RPC options.
- Verified created tasks keep native response fields and do not expose aria2 `bt-tracker` names.

Current conclusion:

Native clients can now create BT tasks with file-selection intent, additional trackers, explicit WebSeed URIs, and ratio/time seeding policy through `/api/v1/tasks`. The old `aria2.addTorrent` and `aria2.addUri` option paths still exist for migration tests and must be retired after native contracts cover the remaining behavior.

Validation:

- `cargo test -p raria-rpc --test native_api task_creation_accepts_native_bt_options`

Next checkpoint:

Continue replacing JSON-RPC BT dispatch tests with native API contracts, then add daemon smoke coverage for native BT creation options.

## Checkpoint 60: Native Torrent Source Backend Selection

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage proving `torrent:` task sources are projected as native torrent sources.
- Fixed engine backend selection so `torrent:` sources create BT jobs instead of range-download jobs.
- Preserved the existing `torrent:base64:` runtime source path used by the BT daemon.

Current conclusion:

Native task creation now routes both magnet and torrent source references to the BitTorrent backend. Native daemon smoke coverage for torrent-byte and torrent-file sources is still needed before the legacy `aria2.addTorrent` dispatch tests can be retired.

Validation:

- `cargo test -p raria-rpc --test native_api task_creation_torrent_source_uses_bt_backend`

Next checkpoint:

Add daemon smoke coverage for native BT creation options and torrent sources, then continue retiring JSON-RPC BT dispatch tests.

## Checkpoint 61: Native Task Transfer Policy API

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage for reading and updating task transfer policy through `/api/v1/tasks/{taskId}/transfer`.
- Added native request and response fields `downloadBytesPerSecondLimit`, `uploadBytesPerSecondLimit`, and `segments`.
- Added an engine-level native transfer policy facade through `TaskId`.
- Updated per-task download limiter state when the native download limit changes.
- Verified the native response does not expose aria2 `max-download-limit` option names.

Current conclusion:

Native clients can now mutate per-task download limits, upload limit options, and segment counts without using `aria2.changeOption`. Queue mutation and source mutation still need typed native endpoints before the runtime option surface can be considered native.

Validation:

- `cargo test -p raria-rpc --test native_api task_transfer_patch_updates_native_runtime_limits`

Next checkpoint:

Add native queue/source mutation endpoints or daemon smoke coverage for the native runtime mutation APIs, then continue retiring JSON-RPC option tests.

## Checkpoint 62: Native Range Source Mutation API

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage for replacing range task sources through `PATCH /api/v1/tasks/{taskId}/sources`.
- Added an engine-level native source replacement facade through `TaskId`.
- Validated replacement sources against supported range protocols.
- Kept BitTorrent source graph mutation out of this endpoint so BT tracker, WebSeed, and torrent metadata controls remain explicit native resources.
- Verified the native response does not expose aria2 `fileIndex` mutation fields.

Current conclusion:

Native clients can now replace HTTP/HTTPS/FTP/FTPS/SFTP source lists without using `aria2.changeUri`. Queue mutation and BT source graph mutation still need native endpoints before the runtime mutation surface is complete.

Validation:

- `cargo test -p raria-rpc --test native_api task_sources_patch_replaces_native_range_sources`

Next checkpoint:

Add native queue mutation or daemon smoke coverage for transfer/source mutation, then continue retiring JSON-RPC mutation tests.

## Checkpoint 63: Native Queue Position API

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage for moving queued tasks through `PATCH /api/v1/tasks/{taskId}/queue`.
- Added `GET /api/v1/tasks/{taskId}/queue` and a native queue-position response with `taskId` and `position`.
- Added native scheduler support for moving opaque `TaskId` values without translating through migration `Gid`.
- Added an engine-level native queue mutation facade that rejects non-queued tasks.
- Verified the native response does not expose aria2 `how` or position-mode fields.

Current conclusion:

Native clients can now change absolute queue position without using `aria2.changePosition`. Relative and end-relative movement are intentionally left out of the native endpoint for now because absolute queue placement is simpler and fits the raria-native resource model.

Validation:

- `cargo test -p raria-rpc --test native_api task_queue_patch_updates_native_waiting_position`

Next checkpoint:

Add daemon smoke coverage for native queue, transfer, and source mutation, then continue removing JSON-RPC mutation tests after native coverage is strong enough.

## Checkpoint 64: Native Global Transfer Policy API

Status: complete

Date: 2026-05-16

Scope completed:

- Added native API contract coverage for reading and updating global transfer policy through `/api/v1/transfer`.
- Added native fields `downloadBytesPerSecondLimit` and `maxActiveTasks`.
- Wired global download limit updates to the shared global limiter.
- Wired max active task updates to the scheduler and notified the daemon worker loop.
- Verified the native response does not expose aria2 `max-overall-download-limit` names.

Current conclusion:

Native clients can now update the global download limiter and active-task concurrency without `aria2.changeGlobalOption`. Global upload-limit mutation still needs a native runtime path because the current engine configuration stores that value immutably.

Validation:

- `cargo test -p raria-rpc --test native_api global_transfer_patch_updates_native_runtime_policy`

Next checkpoint:

Add daemon smoke coverage for native global and task transfer policy, then continue replacing JSON-RPC option tests with native API contracts.

## Checkpoint 65: Native Global Upload Limit Policy

Status: complete

Date: 2026-05-16

Scope completed:

- Extended `/api/v1/transfer` with native `uploadBytesPerSecondLimit`.
- Added mutable engine runtime state for the global upload limit policy.
- Preserved the startup value from `GlobalConfig::max_overall_upload_limit`.
- Extended native API contract coverage to verify PATCH and GET round-trip the upload limit field.

Current conclusion:

The native global transfer policy now carries download limit, upload limit policy, and max active task count. Upload enforcement still needs BT runtime wiring because current range backends only consume download-side throttling.

Validation:

- `cargo test -p raria-rpc --test native_api global_transfer_patch_updates_native_runtime_policy`

Next checkpoint:

Add daemon smoke coverage for native transfer policy and begin wiring upload-limit policy into BT runtime where librqbit exposes suitable controls.

## Checkpoint 29: Engine Native Task Facade

Status: complete

Date: 2026-05-14

Scope completed:

- Added an engine-level native task facade for create, detail projection, list projection, pause, resume, remove, and restart.
- Added focused engine coverage proving the facade creates opaque `TaskId` values and controls lifecycle through native identifiers.
- Moved native API task creation and controls onto the engine facade.
- Removed direct registry mutation and direct `Gid` lookup from the native API task-control handlers.

Current conclusion:

The HTTP native API no longer owns the migration bridge for core task controls. The bridge still exists inside the engine because the runtime registry, scheduler, cancellation registry, and executor still operate on `Gid`.

Validation:

- `cargo test -p raria-core engine::tests::native_task_facade_creates_opaque_task_and_controls_lifecycle`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Move runtime registry ownership toward native `TaskId` while keeping the existing executor bridge internal and covered.

## Checkpoint 30: Job-Owned Native Task IDs

Status: complete

Date: 2026-05-14

Scope completed:

- Added a native `TaskId` field to the current runtime `Job` model.
- Kept old persisted `Job` rows readable by defaulting missing task ids during deserialization.
- Changed native task-row projection to preserve the job-owned task id instead of deriving a migration id.
- Changed native task-row restore to put the persisted task id back onto the restored job.
- Kept the scheduler and executor bridge on `Gid` while moving task identity into the runtime object.

Current conclusion:

The runtime task object now carries the native identity directly. The registry, scheduler, cancellation registry, persistence segment keys, and executor still use `Gid`, so the runtime has not fully moved to native `TaskId` ownership yet.

Validation:

- `cargo test -p raria-core job::tests::job_carries_opaque_native_task_id`
- `cargo test -p raria-core native_persist_tests`
- `cargo test -p raria-core engine::tests::save_session`
- `cargo test -p raria-core engine::tests::engine_restore`
- `cargo test -p raria-core native_projection_tests`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Move registry lookup toward native `TaskId` while keeping `Gid` as the executor bridge key.

## Checkpoint 31: Registry Native Task Lookup

Status: complete

Date: 2026-05-14

Scope completed:

- Added a native `TaskId` index to the in-memory job registry.
- Added registry coverage for insert, update, remove, and restore/load behavior with task ids.
- Updated engine native task lookup to use the registry task-id index before falling back to the temporary bridge index.
- Kept existing `Gid`-based scheduler, cancellation, persistence segment, and executor paths intact.

Current conclusion:

The runtime registry can now resolve native task ids directly. `Gid` remains the execution bridge key, so the next structural migration should target scheduler and executor-facing boundaries.

Validation:

- `cargo test -p raria-core registry::tests`
- `cargo test -p raria-core engine::tests`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Move scheduler and executor-facing boundaries toward native `TaskId`, with `Gid` kept only as a private bridge until removed.

## Checkpoint 32: Native Activation Boundary

Status: complete

Date: 2026-05-14

Scope completed:

- Added native activation methods that expose queued tasks as `TaskId` values.
- Added a native activation handle carrying the public task id, backend kind, cancellation token, and temporary runtime bridge id.
- Added engine coverage proving native activation transitions a task into running state through the native id.
- Updated the daemon activation loop to consume native task ids and activation handles instead of selecting and activating `Gid` values directly.

Current conclusion:

Daemon scheduling is now one layer closer to the native task model. The actual executor functions still require `Gid`, so the activation handle carries a bridge id until executor, cancellation, segment persistence, and BT runtime boundaries are migrated.

Validation:

- `cargo test -p raria-core engine::tests::native_activation_uses_task_id_with_runtime_bridge`
- `cargo test -p raria-core engine::tests`
- `cargo test -p raria-rpc --test native_api`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Move executor, cancellation, and segment persistence boundaries toward native `TaskId`.

## Checkpoint 33: Native Segment Store Seed

Status: complete

Date: 2026-05-14

Scope completed:

- Added a native `redb` segment table keyed by native `TaskId` and segment id.
- Added native segment put, get, list, and remove APIs.
- Added persistence coverage proving native segment checkpoints are isolated by task id.
- Kept the existing `Gid` segment table as the active executor checkpoint path until daemon executor wiring is migrated.

Current conclusion:

The native persistence schema now has a task-id keyed segment checkpoint table. Active checkpoint reads and writes still use the old `Gid` table, so resume is not fully native yet.

Validation:

- `cargo test -p raria-core persist::tests`

Next checkpoint:

Wire daemon range checkpoint writes and reads through native task ids while preserving old segment rows as migration fallback.

## Checkpoint 34: Native Segment Checkpoint Wiring

Status: complete

Date: 2026-05-14

Scope completed:

- Updated daemon range checkpoint restore to prefer native task-id segment rows.
- Kept old `Gid` segment rows as migration fallback when native rows are absent.
- Updated checkpoint callbacks to write native task-id segment rows and old bridge rows during the transition.
- Updated interrupted-download persistence and checkpoint cleanup to include native segment rows.

Current conclusion:

Range checkpoint persistence now uses native task ids on the active daemon path while preserving old segment rows as fallback. A daemon-level interrupted resume smoke test is still needed before the old segment table can be retired.

Validation:

- `cargo test -p raria-core persist::tests`
- `cargo test -p raria-core --test segment_checkpoint`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Add daemon-level interrupted segmented resume coverage against native segment rows, then remove old segment checkpoint dependence from range execution.

## Checkpoint 35: Native Segment Resume Smoke

Status: complete

Date: 2026-05-14

Scope completed:

- Added daemon smoke coverage proving interrupted range downloads write native task-id segment rows.
- Verified a restarted daemon can complete the task after reading native segment checkpoint state.
- Verified the resumed daemon issues an HTTP Range request after restart.
- Fixed native task creation so the requested `segments` field is applied to runtime job options.
- Moved executor checkpoint callbacks before progress publication so externally observed progress is not ahead of persisted checkpoint state.

Current conclusion:

Native segment checkpointing is now covered through a real daemon restart flow. The old `Gid` segment table remains as migration fallback, but the active path can now be validated through native task ids.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_resume_uses_native_segment_rows_after_restart`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-core engine::tests`
- `cargo test -p raria-core persist::tests`
- `cargo test -p raria-core --test segment_checkpoint`

Next checkpoint:

Remove remaining old segment checkpoint dependence from range execution after keeping a focused fallback migration test.

## Checkpoint 36: Native Segment Checkpoint Primary Path

Status: complete

Date: 2026-05-14

Scope completed:

- Stopped double-writing old `Gid` segment rows from the normal range checkpoint callback.
- Kept old `Gid` segment rows as read fallback when native segment rows are absent.
- Kept an exceptional interrupted-write fallback for tasks without a native id.
- Revalidated native daemon segment resume and the older session resume smoke.

Current conclusion:

Range checkpoint writes now use native task-id segment rows as the primary path. The old segment table still exists for migration fallback and focused legacy persistence tests.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_resume_uses_native_segment_rows_after_restart`
- `cargo test -p raria-cli --test session_smoke daemon_resume_after_restart_issues_range_request`
- `cargo test -p raria-core persist::tests`

Next checkpoint:

Add a focused migration fixture for old `Gid` segment fallback, then remove old segment write fallback from runtime code where possible.

## Checkpoint 37: Read-Only Legacy Segment Fallback

Status: complete

Date: 2026-05-14

Scope completed:

- Added focused daemon unit coverage proving old `Gid` segment rows remain a read fallback when native task-id segment rows are absent.
- Added focused daemon unit coverage proving interrupted segment persistence no longer creates old `Gid` segment rows when no runtime task can provide a native task id.
- Removed the remaining old `Gid` interrupted-write fallback from the range daemon path.
- Revalidated the daemon segment test filter, including the native daemon restart resume smoke.

Current conclusion:

Range segment checkpoint writes now stay on native task-id segment rows in the active daemon path. Old `Gid` segment rows remain read-only migration fallback until the native persistence schema can drop the old table after broader migration coverage.

Validation:

- `cargo test -p raria-cli daemon::tests::interrupted_segment_persistence_does_not_create_legacy_rows_without_runtime_job`
- `cargo test -p raria-cli segment`

Next checkpoint:

Continue moving executor, cancellation, and scheduler boundaries from private `Gid` bridge ids to native `TaskId`, then retire old segment-table reads when native schema migration fixtures cover the cutover.

## Checkpoint 38: Native Scheduler Activation Query

Status: complete

Date: 2026-05-14

Scope completed:

- Added scheduler coverage proving stale private bridge IDs in the waiting queue are not exposed through the native activation query.
- Added `Scheduler::native_tasks_to_activate()` as the daemon-facing activation candidate boundary.
- Updated `Engine::activatable_native_tasks()` to delegate to the scheduler's native task-id query instead of mapping `Gid` values itself.

Current conclusion:

The scheduler still stores `Gid` bridge IDs internally, but the activation query used by the daemon now has a native task-id boundary. This is a small step toward moving queue storage and cancellation ownership to `TaskId`.

Validation:

- `cargo test -p raria-core scheduler::tests::native_tasks_to_activate_returns_task_ids_without_stale_queue_entries`
- `cargo test -p raria-core native`

Next checkpoint:

Move cancellation registry access behind native task operations, then migrate scheduler storage from `Gid` to `TaskId` once executor activation can still obtain a private bridge safely.

## Checkpoint 39: Native Cancellation Boundary

Status: complete

Date: 2026-05-14

Scope completed:

- Added engine coverage proving active native tasks can be cancelled without public `Gid` access.
- Added `Engine::cancel_active_native_tasks()` as the daemon shutdown cancellation boundary.
- Updated daemon shutdown to cancel active work through the engine native operation instead of reaching into `cancel_registry` and active `Gid` rows directly.

Current conclusion:

Cancellation storage still uses the private bridge id internally, but daemon shutdown no longer depends on direct `Gid` and cancellation registry access. This narrows another runtime edge toward native task ownership.

Validation:

- `cargo test -p raria-core engine::tests::cancel_active_native_tasks_cancels_running_tokens_without_public_gid_access`
- `cargo test -p raria-core native`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Migrate scheduler queue storage toward `TaskId` while keeping a private bridge resolver for executor activation, then continue reducing public and daemon-level `Gid` dependencies.

## Checkpoint 40: Native Scheduler Queue Storage

Status: complete

Date: 2026-05-14

Scope completed:

- Changed scheduler queue storage from `Gid` values to native `TaskId` values.
- Added native enqueue, dequeue, and waiting queue methods.
- Kept old `Gid` queue methods as migration adapters for legacy tests and JSON-RPC-facing code.
- Updated engine submit, restore, pause, resume, remove, restart, activation, and force-remove paths to use native scheduler queue operations.
- Verified daemon native API smoke still passes with native scheduler queue storage.

Current conclusion:

Queue storage is now native task-id based. The executor still uses a private runtime `Gid` bridge after activation, and legacy queue adapters remain until old public surfaces are removed.

Validation:

- `cargo test -p raria-core scheduler::tests`
- `cargo test -p raria-core engine::tests`
- `cargo test -p raria-core native`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Move more executor-facing operations behind native task service methods, then remove JSON-RPC and legacy queue adapters after native CLI/API coverage replaces them.

## Checkpoint 41: Native Executor State Helpers

Status: complete

Date: 2026-05-14

Scope completed:

- Added engine coverage for native executor-facing helpers that update progress, set runtime connection counts, and complete a task through `TaskId`.
- Added `update_native_progress`, `set_native_runtime_connections`, `complete_native_task`, and `fail_native_task` as native task-id boundaries over the current runtime bridge.
- Updated the daemon range execution path to use native progress, completion, and failure helpers instead of directly mutating terminal runtime job state by `Gid`.
- Revalidated native core tests and daemon native API smoke.

Current conclusion:

The range executor still receives the private runtime bridge id, but more state transitions now enter core through native task-id helpers. This reduces daemon ownership of runtime internals and prepares the executor boundary for a later `TaskId` signature.

Validation:

- `cargo test -p raria-core engine::tests::native_runtime_helpers_update_progress_and_terminal_state`
- `cargo test -p raria-core native`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Introduce a native range execution context that carries `TaskId` plus the temporary bridge id, then move segment planning, checkpoint cleanup, and rate limiter access behind native task service methods.

## Checkpoint 42: Native Range Execution Context

Status: complete

Date: 2026-05-14

Scope completed:

- Added a daemon range execution context carrying the native `TaskId` and temporary runtime bridge id.
- Changed the range download entrypoint to accept the execution context instead of a naked `Gid`.
- Added a runtime guard that rejects mismatched native task id and bridge id pairs.
- Updated daemon activation and focused daemon tests to pass the native execution context.

Current conclusion:

Range execution is still internally bridged through `Gid`, but new daemon range execution code now starts from a native task context. This makes the remaining bridge explicit and easier to remove in later executor migration slices.

Validation:

- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo check --workspace`
- `cargo fmt --check`

Next checkpoint:

Move segment planning, checkpoint cleanup, source retry lookup, and rate limiter access behind native task service methods so the range execution context can stop exposing the bridge id to most daemon code.

## Checkpoint 43: Native Segment and Limiter Helpers

Status: complete

Date: 2026-05-15

Scope completed:

- Added engine coverage for native task-id helpers that obtain per-task rate limiters, persist interrupted segment checkpoints, and clean segment checkpoints.
- Added `native_task_rate_limiter`, `persist_native_interrupted_segments`, and `cleanup_native_segment_checkpoints` to move daemon range state handling behind the core native task boundary.
- Updated daemon range execution to use the native task-id helpers for rate limiter access, interrupted checkpoint persistence, and checkpoint cleanup.
- Kept old `Gid` segment rows as read-only migration fallback while making active writes and cleanup enter through native helpers.

Current conclusion:

Range execution still carries a temporary runtime bridge id for executor compatibility, but segment persistence and rate limiter access no longer require daemon-owned store or limiter plumbing. The remaining high-value range bridge points are segment planning metadata, source retry lookup, output path updates, and old event/log fields.

Validation:

- `cargo test -p raria-core engine::tests::native_runtime_helpers_manage_rate_limiter_and_segment_state`
- `cargo test -p raria-cli segment`

Next checkpoint:

Move source retry lookup and output path/runtime metadata updates behind native task service methods, then continue reducing direct daemon registry access.

## Checkpoint 44: Native Source and Output Helpers

Status: complete

Date: 2026-05-15

Scope completed:

- Added engine coverage for native task-id helpers that select the next source, apply a remote output filename, and reset per-source retry state.
- Added `native_task_next_source`, `set_native_output_filename_if_unset`, and `reset_native_task_for_next_source`.
- Updated daemon range execution to use native helpers for mirror selection, output filename updates, retry checks, and mirror retry reset.
- Removed daemon-local source occurrence selection tests after moving the behavior into core.

Current conclusion:

Range execution still has a private runtime bridge id for executor compatibility, but source selection and output metadata updates are now owned by core native task helpers. Remaining direct daemon registry access is concentrated around segment planning inputs, source failure events, and the current `Job` snapshot used to build protocol context.

Validation:

- `cargo test -p raria-core engine::tests::native_runtime_helpers_manage_sources_output_and_retry_reset`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`

Next checkpoint:

Move range segment planning metadata updates and source-failure publication behind native task service methods, then reassess remaining daemon `Gid` bridge usage.

## Checkpoint 45: Native Segment Plan and Source Failure Helpers

Status: complete

Date: 2026-05-15

Scope completed:

- Added engine coverage for native task-id helpers that update segment planning metadata and publish source-failure events.
- Added `set_native_segment_plan_metadata` and `source_failed_native_task`.
- Changed the legacy `source_failed` bridge to delegate through the native helper.
- Updated daemon range execution to use native helpers for segment plan metadata updates and source-failure publication.

Current conclusion:

Range segment planning still happens in the daemon, but its runtime metadata update now enters core through a native task-id boundary. Source-failure publication also has a native task-id entrypoint, reducing another direct `Gid` dependency in daemon execution.

Validation:

- `cargo test -p raria-core engine::tests::native_runtime_helpers_update_segment_plan_and_source_failure`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`

Next checkpoint:

Move the remaining range planning checkpoint restore/write construction into a native planning helper, then reassess whether the range executor can receive `TaskId` as its primary identifier.

## Checkpoint 46: Native Segment Planning Helper

Status: complete

Date: 2026-05-15

Scope completed:

- Added core coverage proving native segment planning restores read-only legacy `Gid` checkpoint rows when native rows are absent and writes new checkpoint progress only to native task-id segment rows.
- Added `NativeSegmentPlanningInput`, `NativeSegmentPlan`, `plan_native_segments`, `checkpoint_native_segment`, and `native_segment_checkpoint_callback`.
- Updated daemon range planning to call the native core helper for split calculation, metadata updates, checkpoint restore, and checkpoint callback construction.
- Kept legacy `Gid` segment rows as fallback-only migration input while continuing active writes through native task ids.

Current conclusion:

Range segment planning and checkpoint callback construction now live behind core native task-id helpers. Daemon range execution still snapshots `Job` for protocol options and still carries a temporary runtime bridge id for executor compatibility, but it no longer owns store-level segment restore/write construction.

Validation:

- `cargo test -p raria-core engine::tests::native_segment_planning_restores_checkpoints_and_writes_native_rows`
- `cargo test -p raria-cli daemon::tests::legacy_gid_segment_rows_remain_read_fallback_for_resume`
- `cargo test -p raria-cli daemon::tests::interrupted_segment_persistence_does_not_create_legacy_rows`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo fmt --all --check`
- `cargo check --workspace --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`

Next checkpoint:

Move the range executor activation inputs from daemon `Job` snapshots toward a native range task execution descriptor, then reduce the remaining runtime bridge id exposure.

## Checkpoint 47: Native Range Execution Descriptor

Status: complete

Date: 2026-05-16

Scope completed:

- Added core coverage for an executor-facing native range task descriptor that exposes output path, explicit output naming, per-task limits, per-task headers, per-task authentication, whole-file checksum, and piece checksum metadata without requiring daemon code to snapshot `Job`.
- Added `NativeRangeExecutionTask` and `native_range_execution_task`.
- Updated daemon range execution context construction to consume the native descriptor for backend context, output path resolution, segment planning inputs, rate limits, and checksum verification.
- Kept the temporary runtime bridge id only where current events, logs, and legacy executor activation still require it.

Current conclusion:

Daemon range execution no longer reaches into the registry for the primary task option snapshot. The remaining range bridge is narrower: `Gid` is still present for log/event compatibility and for old activation plumbing, while task execution settings now enter through a native core descriptor.

Validation:

- `cargo test -p raria-core engine::tests::native_range_execution_descriptor_exposes_runtime_inputs_without_job_snapshot`
- `cargo test -p raria-cli daemon::tests::legacy_gid_segment_rows_remain_read_fallback_for_resume`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo check --workspace --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`

Next checkpoint:

Move completion, failure, and structured log identifiers toward native task ids so the daemon range path can stop deriving task ids from runtime `Gid` during terminal transitions.

## Checkpoint 48: Native Range Terminal Paths

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon coverage proving range completion accepts the native task id as the terminal authority.
- Changed `finalize_complete` to receive `TaskId` directly instead of deriving it from the runtime bridge id.
- Updated range success, cached-not-modified success, mirror failure, and all-mirrors-failed paths to call native terminal helpers with the execution context task id.
- Kept the temporary runtime bridge id for current event/log compatibility only.

Current conclusion:

Range terminal transitions no longer use `Gid` to recover the native task id. The remaining bridge surface is concentrated in legacy event payloads, structured log fields, and executor activation plumbing.

Validation:

- `cargo test -p raria-cli daemon::tests::finalize_complete_uses_native_task_id_as_terminal_authority`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Move daemon structured log fields and event projection toward native task ids, then reassess whether `RangeExecutionContext` can stop exposing `runtime_gid` outside legacy event compatibility.

## Checkpoint 49: Native Range Structured Log Fields

Status: complete

Date: 2026-05-16

Scope completed:

- Added daemon coverage proving range structured log fields include the native task id.
- Added a shared `range_structured_fields` helper for daemon range JSONL events.
- Updated range start, mirror retry, and integrity-failure structured logs to include `task_id` alongside the temporary runtime `gid`.

Current conclusion:

Range daemon JSONL records now carry native task correlation. The remaining `gid` exposure is still needed for old event payloads and bridge-era diagnostics until the event bus is made fully native.

Validation:

- `cargo test -p raria-cli daemon::tests::range_structured_fields_include_native_task_id`
- `cargo test -p raria-cli daemon::tests::finalize_complete_uses_native_task_id_as_terminal_authority`
- `cargo test -p raria-cli daemon::tests::mirror_failover_publishes_source_failed_event_before_completion`
- `cargo test -p raria-cli --test native_api_smoke`

Next checkpoint:

Move native event payloads and range progress/source-failure projections further away from legacy `DownloadEvent` GID-only fields.

## Checkpoint 28: Daemon Native API Auth from raria.toml

Status: complete

Date: 2026-05-14

Scope completed:

- Added daemon smoke coverage proving `api.auth_token_file` in `raria.toml` protects `/api/v1` routes.
- Preserved unauthenticated `/api/v1/health` for readiness checks.
- Carried the native API bearer token from `RariaConfig` into the runtime `GlobalConfig`.
- Wired the shared daemon listener's native API router to the configured bearer token.
- Added config conversion coverage proving `to_global_config()` keeps the native API token.

Current conclusion:

Daemon mode now honors native `raria.toml` bearer authentication for the native API. The remaining JSON-RPC secret path is still present as migration scaffolding and must be removed once native control coverage replaces the old surface.

Validation:

- `cargo test -p raria-cli --test native_api_smoke daemon_native_api_uses_raria_toml_bearer_auth`
- `cargo test -p raria-core --test native_config`
- `cargo test -p raria-cli --test native_api_smoke`
- `cargo test -p raria-rpc --test native_api`

Next checkpoint:

Continue removing migration identifier assumptions from native API events and persistence fixtures, then move runtime registry ownership toward `TaskId`.
