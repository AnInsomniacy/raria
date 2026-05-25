# Native Surface Audit

This file records the CM-003 baseline. It is a deletion map, not a
compatibility plan. raria keeps useful downloader behavior only when it is
exposed through native API resources, native WebSocket events, `raria.toml`,
native CLI names, opaque task identifiers, and structured logs.

## Native API Resources

The retained product contract is `/api/v1`. Current native routes are:

| Resource | Methods | Capability owner | Decision |
| --- | --- | --- | --- |
| `/api/v1/health` | GET | daemon health | Retain |
| `/api/v1/config` | GET | runtime config projection | Retain after CM-006 renames runtime fields |
| `/api/v1/daemon/shutdown` | POST | daemon lifecycle | Retain |
| `/api/v1/events` | GET WebSocket | native event stream | Retain |
| `/api/v1/session/save` | POST | session persistence | Retain |
| `/api/v1/stats` | GET | native stats | Retain |
| `/api/v1/transfer` | GET, PATCH | global transfer policy | Retain |
| `/api/v1/tasks` | GET, POST | task creation and listing | Retain |
| `/api/v1/tasks/{taskId}` | GET, DELETE | task detail and removal | Retain |
| `/api/v1/tasks/{taskId}/pause` | POST | lifecycle control | Retain |
| `/api/v1/tasks/{taskId}/resume` | POST | lifecycle control | Retain |
| `/api/v1/tasks/{taskId}/restart` | POST | lifecycle control | Retain |
| `/api/v1/tasks/{taskId}/queue` | GET, PATCH | queue position | Retain |
| `/api/v1/tasks/{taskId}/files` | GET, PATCH | file selection | Retain |
| `/api/v1/tasks/{taskId}/sources` | GET, PATCH | source graph and mirrors | Retain |
| `/api/v1/tasks/{taskId}/trackers` | GET, PATCH | BitTorrent trackers | Retain |
| `/api/v1/tasks/{taskId}/peers` | GET | BitTorrent peers | Retain |
| `/api/v1/tasks/{taskId}/bt/seeding` | GET, PATCH | BitTorrent seeding policy | Retain |
| `/api/v1/tasks/{taskId}/transfer` | GET, PATCH | per-task transfer policy | Retain |

CM-005 and CM-020 closed the native API/event replacement path. Native
resources and `/api/v1/events` are the public contract. JSON-RPC listener
merge, old method routes, and old notification delivery are deleted.

## Native Event Stream

The retained event envelope is `NativeEvent` with `version`, `sequence`,
`time`, `type`, optional `taskId`, and typed `data`. Current retained event
types are `task.created`, `task.started`, `task.paused`, `task.resumed`,
`task.completed`, `task.failed`, `task.removed`, `task.progress`,
`task.source.failed`, `task.bt.metadata.resolved`,
`task.bt.seeding.started`, `task.bt.peer.updated`, and
`task.bt.tracker.updated`.

The old event projection and same-socket notification delivery were deleted
after native event coverage replaced lifecycle, progress, source failure,
BitTorrent metadata, seeding, peer, and tracker evidence. `DownloadEvent`
remains private runtime bridge input only where core and daemon internals still
need it.

## JSON-RPC Deletion Map

CM-020 deleted the JSON-RPC server contract. Removed surfaces include `/`,
`/jsonrpc`, the jsonrpsee direct dependency, RPC handlers, method facades,
system methods, token-in-params auth, same-socket notification delivery, and
old method or notification names.

Useful behavior is retained through native resources for task creation, task
removal, pause, resume, task status, list by lifecycle, global stats, session
save, shutdown, per-task transfer policy, global transfer policy, queue
position, file selection, source mutation, tracker mutation, peer projection,
and event delivery.

## CLI And Configuration Surfaces

Retained configuration is strict `raria.toml` from `native_config.rs`.
Retained CLI names are native names such as `daemon --api-port`,
`--on-task-start`, `--on-task-complete`, and `--on-task-fail`.

Runtime `GlobalConfig` now uses native names for retained transfer,
directory, proxy, cookie, retry, resume, segment, BitTorrent, and daemon
lifecycle policy. Task-level public fields use native task creation names.

## Tests And Documentation

Retained tests are high-value native contract tests, daemon smoke tests,
native config tests, protocol smoke tests, persistence tests, and targeted
regression tests. Tests that existed only for old wire format, JSON-RPC
behavior, system method discovery, token-in-params auth, or old notification
shape were deleted instead of translated mechanically.

Product docs after CM-021 describe only native API resources, native events,
native CLI, native completion, and native configuration.

## Reproducible Stale-Surface Baseline

Use these searches as the CM-020 stale-surface baseline:

```bash
rg -n "jsonrpsee|JSON-RPC|json-rpc|aria2\\.|AriaNg|Motrix|system\\.multicall|addUri|tellStatus|tellActive|tellWaiting|tellStopped" crates README.md docs/core-modernization
rg -n "\\bGid\\b|\\bgid\\b" crates README.md docs/core-modernization
rg -n "compat|parity|legacy|rpc_secret|rpc_allow_origin_all|rpc-port|rpc-secret" crates README.md docs/core-modernization
```

Expected post-CM-020 findings are historical tracker evidence only for old
public surfaces. Runtime-private `Gid`, `Job`, `EventBus`, and `DownloadEvent`
references may remain only where they are documented private implementation
details and never public product contract.
