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

Known CM-005 and later gaps are native API contract documentation, exact
request and response schema stability, native CORS/origin policy, and removal
of the shared listener merge with the JSON-RPC server.

## Native Event Stream

The retained event envelope is `NativeEvent` with `version`, `sequence`,
`time`, `type`, optional `taskId`, and typed `data`. Current retained event
types are `task.created`, `task.started`, `task.paused`, `task.resumed`,
`task.completed`, `task.failed`, `task.removed`, `task.progress`,
`task.source.failed`, `task.bt.metadata.resolved`,
`task.bt.seeding.started`, `task.bt.peer.updated`, and
`task.bt.tracker.updated`.

`crates/raria-rpc/src/events.rs`, `ws_event_push_loop`, `DownloadEvent`
notification projection, and same-socket JSON-RPC notification delivery are
legacy surfaces. Delete them after native event coverage fully replaces useful
lifecycle, progress, source failure, BitTorrent metadata, seeding, peer, and
tracker evidence.

## JSON-RPC Deletion Map

Delete the JSON-RPC server contract. This includes `/`, `/jsonrpc`,
`jsonrpsee`, `RpcHandler`, `Aria2Rpc`, `RpcOptions`, `system.multicall`,
`system.listMethods`, `system.listNotifications`, token-in-params auth,
same-socket notification delivery, and aria2 method or notification names.

Useful behavior that must already have or receive native coverage before
deletion is task creation, task removal, pause, resume, task status, list by
lifecycle, global stats, session save, shutdown, per-task options, global
transfer policy, queue position, file selection, source mutation, tracker
mutation, peer projection, and event delivery. These map to CM-005 through
CM-019, then CM-020 deletes the legacy surface.

## CLI And Configuration Surfaces

Retained configuration is strict `raria.toml` from `native_config.rs`.
Retained CLI names are native names such as `daemon --api-port`,
`--on-task-start`, `--on-task-complete`, and `--on-task-fail`.

Runtime `GlobalConfig` still exposes transitional fields that must be renamed
or removed in CM-006 and dependent checkpoints: `rpc_secret`,
`rpc_allow_origin_all`, `dir`, `out`, `split`,
`max_overall_download_limit`, `max_overall_upload_limit`,
`max_connection_per_server`, `continue_download`, `min_split_size`,
`lowest_speed_limit`, `max_file_not_found`, `max_tries`, `retry_wait`,
`all_proxy`, `http_passwd`, `cookie_file`, `save_cookie_file`,
`bt_selected_files`, `bt_trackers`, `seed_ratio`, and `seed_time`.

Some of these names describe useful behavior. Keep the behavior through
native names. Delete aria2-shaped names and comments once callers move to the
native schema.

## Tests And Documentation

Retain high-value native tests in `crates/raria-rpc/tests/native_api.rs`,
daemon smoke tests, native config tests, protocol smoke tests, persistence
tests, and targeted regression tests. Trim any test that exists only to prove
aria2 wire format, JSON-RPC behavior, system method discovery, token-in-params
auth, or legacy notification shape.

Current deletion candidates are `crates/raria-rpc/tests/rpc_parity.rs`,
`multicall_parity.rs`, `options_parity.rs`, `ws_parity.rs`, `ws_push.rs`,
`rpc_secret.rs`, `global_stat.rs`, `http_cors.rs`, and
`legacy_surface.rs`. Migrate only the useful behavior that is not already
covered by native tests. Do not translate parity scaffolding mechanically.

README.md still mentions old JSON-RPC as a temporary harness. Remove that
wording when CM-020 deletes the legacy implementation. Product docs after
CM-021 must describe only native API resources, native events, native CLI, and
native configuration.

## Reproducible Stale-Surface Baseline

Use these searches as the CM-020 stale-surface baseline:

```bash
rg -n "jsonrpsee|JSON-RPC|json-rpc|aria2\\.|AriaNg|Motrix|system\\.multicall|addUri|tellStatus|tellActive|tellWaiting|tellStopped" crates README.md docs/core-modernization
rg -n "\\bGid\\b|\\bgid\\b" crates README.md docs/core-modernization
rg -n "compat|parity|legacy|rpc_secret|rpc_allow_origin_all|rpc-port|rpc-secret" crates README.md docs/core-modernization
```

Expected CM-003 baseline findings are active JSON-RPC implementation in
`crates/raria-rpc/src/server.rs`, `methods.rs`, `events.rs`, and `facade.rs`;
aria2 compatibility tests under `crates/raria-rpc/tests`; transitional
`Gid` and `Job` ownership in core runtime paths; transitional runtime config
field names; and README wording that describes JSON-RPC as temporary.
