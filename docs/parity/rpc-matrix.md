# RPC Method Parity Matrix: raria vs aria2 1.37.0

> Updated: 2026-04-09 | Baseline: aria2 1.37.0

## Legend

| Status | Meaning |
|--------|---------|
| ✅ done | Implemented and returning correct response format |
| 🔧 partial | Registered but missing fields or incomplete behavior |
| ❌ stub | Returns error or placeholder |
| ⏸️ deferred | Will not implement in v1 |

---

## Download Control Methods

| Method | aria2 | raria | Status | Test Coverage |
|--------|-------|-------|--------|---------------|
| `aria2.addUri` | ✅ | ✅ | ✅ done | Unit + RPC integration |
| `aria2.addTorrent` | ✅ | ❌ | ❌ stub | Returns "not implemented" |
| `aria2.addMetalink` | ✅ | ❌ | ❌ stub | Returns "not implemented" |
| `aria2.remove` | ✅ | ✅ | ✅ done | Unit test |
| `aria2.forceRemove` | ✅ | ✅ | ✅ done | Unit test |
| `aria2.pause` | ✅ | ✅ | ✅ done | Unit test |
| `aria2.pauseAll` | ✅ | ✅ | ✅ done | Unit test |
| `aria2.forcePause` | ✅ | ✅ | ✅ done | Unit test |
| `aria2.forcePauseAll` | ✅ | ✅ | ✅ done | Unit test |
| `aria2.unpause` | ✅ | ✅ | ✅ done | Unit test |
| `aria2.unpauseAll` | ✅ | ✅ | ✅ done | Unit test |

## Query Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `aria2.tellStatus` | ✅ | ✅ | ✅ done | String-typed numbers verified |
| `aria2.getUris` | ✅ | ✅ | ✅ done | |
| `aria2.getFiles` | ✅ | ✅ | ✅ done | |
| `aria2.getPeers` | ✅ | 🔧 | 🔧 partial | Returns empty for non-BT |
| `aria2.getServers` | ✅ | 🔧 | 🔧 partial | |
| `aria2.tellActive` | ✅ | ✅ | ✅ done | |
| `aria2.tellWaiting` | ✅ | ✅ | ✅ done | With offset/num |
| `aria2.tellStopped` | ✅ | ✅ | ✅ done | |
| `aria2.getGlobalStat` | ✅ | ✅ | ✅ done | String-typed numbers verified |
| `aria2.getVersion` | ✅ | ✅ | ✅ done | Returns raria version |
| `aria2.getSessionInfo` | ✅ | ✅ | ✅ done | |

## Configuration Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `aria2.changeOption` | ✅ | 🔧 | 🔧 partial | Registered but TODO: doesn't actually apply |
| `aria2.getOption` | ✅ | ✅ | ✅ done | |
| `aria2.changeGlobalOption` | ✅ | 🔧 | 🔧 partial | TODO: doesn't actually apply |
| `aria2.getGlobalOption` | ✅ | ✅ | ✅ done | |
| `aria2.changePosition` | ✅ | ✅ | ✅ done | POS_SET/POS_CUR/POS_END |

## Session Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `aria2.purgeDownloadResult` | ✅ | ✅ | ✅ done | |
| `aria2.removeDownloadResult` | ✅ | ✅ | ✅ done | |
| `aria2.saveSession` | ✅ | 🔧 | 🔧 partial | Returns OK but no actual save |
| `aria2.shutdown` | ✅ | ✅ | ✅ done | |
| `aria2.forceShutdown` | ✅ | ✅ | ✅ done | |

## System Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `system.multicall` | ✅ | ❌ | ❌ stub | Critical for AriaNg |
| `system.listMethods` | ✅ | ❌ | ❌ stub | |
| `system.listNotifications` | ✅ | ❌ | ❌ stub | |

## WebSocket Notifications

| Notification | aria2 | raria | Status | Notes |
|-------------|-------|-------|--------|-------|
| `aria2.onDownloadStart` | ✅ | 🔧 | 🔧 partial | Event mapped, not yet pushed to WS |
| `aria2.onDownloadPause` | ✅ | 🔧 | 🔧 partial | Event mapped, not yet pushed to WS |
| `aria2.onDownloadStop` | ✅ | 🔧 | 🔧 partial | Event mapped, not yet pushed to WS |
| `aria2.onDownloadComplete` | ✅ | 🔧 | 🔧 partial | Event mapped, not yet pushed to WS |
| `aria2.onDownloadError` | ✅ | 🔧 | 🔧 partial | Event mapped, not yet pushed to WS |
| `aria2.onBtDownloadComplete` | ✅ | ❌ | ❌ stub | BT-GAP-005 |

## Security

| Feature | aria2 | raria | Status | Notes |
|---------|-------|-------|--------|-------|
| RPC secret token | ✅ | ❌ | ❌ stub | --rpc-secret |
| Token-free methods | ✅ | ❌ | ❌ stub | listMethods, listNotifications |
