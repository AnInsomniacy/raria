# Core Ownership Audit

This file records the CM-004 baseline. It separates the native raria model
from temporary runtime bridges so later checkpoints can delete legacy
ownership without losing downloader behavior.

## Target Ownership

`TaskId` is the public identity. `Gid` is a temporary private bridge until the
runtime no longer needs `Job` as its primary row. Native API responses, event
payloads, CLI output, persistence schemas, logs meant for product use, and
future client contracts must use `TaskId`.

`Job` currently owns too much state. The target model is native task metadata,
native lifecycle, native transfer policy, native source graph, native file
state, native piece state, native BitTorrent projections, and native
persistence rows. Protocol executors may receive compact execution descriptors
but should not own public state.

## Identity Bridge

Current bridge points are:

| Area | Current owner | Target owner | Deletion checkpoint |
| --- | --- | --- | --- |
| Public task id | `TaskId` in `native.rs` and `Job.task_id` | `TaskId` only | CM-007 |
| Runtime id | `Gid` in `job.rs`, `registry.rs`, `engine.rs` | private executor handle or deleted | CM-007, CM-008 |
| Bridge index | `JobRegistry.by_task_id` | one native task registry | CM-007 |
| Public projections | `NativeTaskSummary::from_runtime_job` | native task row and runtime snapshot | CM-008 |
| API lookup | API parses `TaskId`, engine resolves to `Gid` | direct native task lookup | CM-007, CM-008 |
| Logs | several structured logs still emit `gid` | taskId correlation | CM-019 |

CM-007 removed the duplicate task-id index. `JobRegistry.by_task_id` is the
only current TaskId-to-runtime bridge until CM-008 replaces the Job-driven
runtime model.

## Runtime Model

Current engine control still flows through `Job` and `Gid` for submission,
pause, resume, remove, activation, completion, failure, rate limiters, result
purge, queue movement, and session save. Native methods mostly wrap these
operations after resolving `TaskId` to `Gid`.

Target CM-008 runtime ownership is a native task service with `TaskId`
operations as the primary path. `Job` can shrink into a private executor row
or be replaced by native structs. Batch operations such as pause-all,
resume-all, purge, queue mutation, restart, and result removal must be
expressed as native lifecycle operations, not aria2 parity helpers.

`Scheduler` already stores `TaskId` in the waiting queue. Its old
`jobs_to_activate` bridge can be deleted once daemon and executor loops use
`activatable_native_tasks` only. `CancelRegistry` and rate limiters are still
keyed by `Gid`; move them behind native task keys or a private activation id
in CM-008 and CM-018.

## Persistence Ownership

redb remains the storage engine. Current tables are mixed:

| Table | Current role | Target decision |
| --- | --- | --- |
| `native_metadata` | native store metadata | Retain |
| `native_tasks` | versioned native task rows keyed by `TaskId` | Retain and expand |
| `native_segments` | native segment checkpoints keyed by `TaskId` | Retain |
| `jobs` | serialized `Job` keyed by raw `Gid` | Delete after native rows carry full state |
| `segments` | segment checkpoints keyed by `Gid` | Delete after native segment fallback is removed |
| `job_options` | serialized `JobOptions` keyed by raw `Gid` | Delete or fold into native task rows |
| `global_state` | miscellaneous old key-value state | Retain only native metadata that survives CM-009 |

`restore`, `persist_job`, `save_session`, and tests still fall back to direct
`Job` rows. CM-009 must add native schema fixtures, remove legacy row
deserialization tests, and make unsupported old rows fail with clear native
errors instead of silently restoring aria2-era storage shapes.

## Event Ownership

`NativeEventBus` is the retained event source. `EventBus` and `DownloadEvent`
exist for legacy JSON-RPC notification projection and some internal tests.
Native lifecycle, progress, source failure, BitTorrent metadata, seeding,
peer, and tracker events must publish through `NativeEventBus` only after
CM-005. Delete aria2 notification projection in CM-020.

## BitTorrent Runtime Bridge

`raria-bt` correctly owns librqbit session interaction, torrent ingress,
metadata snapshots, file selection calls, peer snapshots, tracker snapshots,
fastresume directory binding, DHT configuration, WebSeed pre-download, and
library-facing piece strategy. raria owns task identity, task lifecycle,
source and policy inputs, public projections, persistence references,
seeding policy, upload/download limits, and cleanup behavior.

Current bridge debt is `BtHandle.gid`, BT status syncing into `Job`,
BT file and peer caches on `Job`, tracker and seeding policy stored in
`JobOptions`, and `persist_bt_job` writing direct `Job` rows. CM-016 and
CM-017 should keep librqbit as the engine and move raria-owned state into
native task rows and native snapshots without wrapping librqbit internals.

## Refactor Order

CM-007 should remove public `Gid` behavior and unify identity lookup.
CM-008 should move lifecycle, scheduler activation, cancellation, status
projection, and result operations to native task services. CM-009 should make
native redb rows the only session truth. CM-005 and CM-020 should remove the
legacy event and JSON-RPC paths after native coverage is sufficient. CM-019
should finish taskId logging and security surface cleanup.

No implementation cleanup should happen only for appearance. Each deletion
must remove a legacy owner, replace it with a native owner, or document a
proven limitation.

## Reproducible Ownership Searches

Use these searches to refresh the ownership map:

```bash
rg -n "\\bGid\\b|\\bgid\\b|from_runtime_job|runtime_bridge_id" crates/raria-core crates/raria-cli crates/raria-rpc
rg -n "put_job|get_job|list_jobs|remove_job|put_segment|get_segment|list_segments|remove_segments|native_tasks|native_segments" crates/raria-core/src/persist.rs crates/raria-core/tests
rg -n "EventBus|DownloadEvent|NativeEventBus|NativeEvent" crates/raria-core crates/raria-rpc crates/raria-cli
rg -n "BtHandle|BtStatus|bt_files|bt_peers|persist_bt_job|librqbit|ManagedTorrent|Session" crates/raria-bt crates/raria-cli/src/bt_runtime.rs crates/raria-core
```
