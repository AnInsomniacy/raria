# raria Modernization Progress Log

This log tracks checkpoints for the long-running modernization goal. It is intentionally concise and evidence-oriented.

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
