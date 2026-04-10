# RPC Method Parity Matrix: raria vs aria2 1.37.0

> Updated: 2026-04-09 | Baseline: aria2 1.37.0

## Legend

| Status | Meaning |
|--------|---------|
| `has_code` | Method or support code exists but is not fully wired into real behavior |
| `wired` | Available on the real server path, but behavior is still incomplete |
| `tested` | Automated coverage exists and is passing |
| `client_verified` | Verified through real daemon/RPC/WebSocket smoke or client flows |
| `gap` | Known incompatibility or intentionally unsupported |

---

## Download Control Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `aria2.addUri` | ✅ | ✅ | `client_verified` | Covered by daemon RPC smoke |
| `aria2.addTorrent` | ✅ | ✅ | `tested` | BT dispatch tests exercise job creation |
| `aria2.addMetalink` | ✅ | ✅ | `tested` | Metalink dispatch tests exist |
| `aria2.remove` | ✅ | ✅ | `tested` | Unit coverage |
| `aria2.forceRemove` | ✅ | ✅ | `tested` | Unit coverage |
| `aria2.pause` | ✅ | ✅ | `tested` | Unit coverage |
| `aria2.pauseAll` | ✅ | ✅ | `tested` | Unit coverage |
| `aria2.forcePause` | ✅ | ✅ | `tested` | Unit coverage |
| `aria2.forcePauseAll` | ✅ | ✅ | `tested` | Unit coverage |
| `aria2.unpause` | ✅ | ✅ | `tested` | Unit coverage |
| `aria2.unpauseAll` | ✅ | ✅ | `tested` | Unit coverage |

## Query Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `aria2.tellStatus` | ✅ | ✅ | `client_verified` | RPC smoke and parity tests |
| `aria2.getUris` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.getFiles` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.getPeers` | ✅ | ✅ | `tested` | HTTP path returns empty and BT cached peer detail is covered by RPC tests |
| `aria2.getServers` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.tellActive` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.tellWaiting` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.tellStopped` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.getGlobalStat` | ✅ | ✅ | `tested` | Parity tests |
| `aria2.getVersion` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.getSessionInfo` | ✅ | ✅ | `tested` | RPC tests |

## Configuration Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `aria2.changeOption` | ✅ | ✅ | `tested` | Options parity tests cover BT trackers and seeding controls in addition to core fields |
| `aria2.getOption` | ✅ | ✅ | `tested` | Options parity tests |
| `aria2.changeGlobalOption` | ✅ | ✅ | `client_verified` | Daemon RPC smoke verifies live download-limit mutation on active jobs |
| `aria2.getGlobalOption` | ✅ | ✅ | `tested` | Options parity tests |
| `aria2.changePosition` | ✅ | ✅ | `tested` | RPC tests |

## Session Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `aria2.purgeDownloadResult` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.removeDownloadResult` | ✅ | ✅ | `tested` | RPC tests |
| `aria2.saveSession` | ✅ | ✅ | `tested` | Dedicated daemon smoke verifies direct RPC-triggered session persistence |
| `aria2.shutdown` | ✅ | ✅ | `client_verified` | Daemon smoke verifies graceful shutdown |
| `aria2.forceShutdown` | ✅ | ✅ | `tested` | RPC tests |

## System Methods

| Method | aria2 | raria | Status | Notes |
|--------|-------|-------|--------|-------|
| `system.multicall` | ✅ | ✅ | `tested` | Multicall parity tests |
| `system.listMethods` | ✅ | ✅ | `tested` | Multicall parity tests |
| `system.listNotifications` | ✅ | ✅ | `tested` | Multicall parity tests |

## WebSocket Notifications

| Notification | aria2 | raria | Status | Notes |
|-------------|-------|-------|--------|-------|
| `aria2.onDownloadStart` | ✅ | ✅ | `tested` | WS push tests and parity tests |
| `aria2.onDownloadPause` | ✅ | ✅ | `tested` | Event mapping tests |
| `aria2.onDownloadStop` | ✅ | ✅ | `tested` | Event mapping tests |
| `aria2.onDownloadComplete` | ✅ | ✅ | `tested` | WS push and mapping tests |
| `aria2.onDownloadError` | ✅ | ✅ | `tested` | Event mapping tests |
| `aria2.onBtDownloadComplete` | ✅ | ❌ | `gap` | BT-GAP-005 |

## Security

| Feature | aria2 | raria | Status | Notes |
|---------|-------|-------|--------|-------|
| RPC secret token | ✅ | ✅ | `tested` | Dedicated RPC secret tests |
| Token-free system methods when no secret is configured | ✅ | ✅ | `tested` | Server parity tests |
