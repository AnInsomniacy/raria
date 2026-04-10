# Protocol Parity Matrix: raria vs aria2 1.37.0

> Updated: 2026-04-09 | Baseline: aria2 1.37.0

## Legend

| Status | Meaning |
|--------|---------|
| `has_code` | Implementation code exists, but the real hot path does not consume it |
| `wired` | Connected to the production path, but not yet validated by dedicated automated coverage |
| `tested` | Covered by automated tests and passing |
| `client_verified` | Verified through real end-to-end behavior or real client flows |
| `gap` | Known incompatibility or intentionally unsupported behavior |

---

## HTTP/HTTPS

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic download | ✅ | ✅ | `client_verified` | Binary-path single-download smoke test |
| Range requests (segmented) | ✅ | ✅ | `tested` | Executor integration tests |
| Resume (partial download) | ✅ | ✅ | `wired` | Segment checkpoints exist; full resume semantics still incomplete |
| Content-Disposition filename | ✅ | ✅ | `client_verified` | Single-download hot path now honors `suggested_filename` |
| ETag / If-Range | ✅ | ✅ | `wired` | Probe/open path connected; resume semantics still broader than current verification |
| Redirect following | ✅ | ✅ | `tested` | reqwest default behavior |
| Redirect policy config | ✅ | ✅ | `client_verified` | `--max-redirect` verified on the binary path |
| HTTP proxy | ✅ | ✅ | `tested` | HTTP config smoke test covers `no_proxy` bypass |
| HTTPS proxy | ✅ | ✅ | `wired` | Connected via `HttpBackendConfig`; no dedicated integration test yet |
| SOCKS5 proxy | ✅ | ✅ | `has_code` | reqwest feature enabled, no product path coverage yet |
| Cookie file (Netscape) | ✅ | ✅ | `tested` | Hot path verified by integration smoke |
| `.netrc` auth | ✅ | ✅ | `client_verified` | `--netrc-path` verified on the binary path |
| `no-netrc` credential suppression | ✅ | ✅ | `client_verified` | `--no-netrc` verified on the binary path |
| Custom headers | ✅ | ✅ | `client_verified` | RPC path covered and CLI binary path verified |
| TLS CA certificate | ✅ | ✅ | `wired` | Connected to reqwest builder, no dedicated smoke yet |
| Disable cert verification | ✅ | ✅ | `wired` | Connected to reqwest builder |
| Basic auth | ✅ | ✅ | `client_verified` | Verified on both single-download CLI and daemon/RPC paths |
| Digest auth | ✅ | ❌ | `has_code` | Not implemented |
| Metalink/HTTP (RFC 6249) | ✅ | ❌ | `has_code` | Not implemented |
| Request timeout | ✅ | ✅ | `client_verified` | Single-download CLI timeout path verified |
| Connect timeout | ✅ | ✅ | `client_verified` | Single-download CLI connect-timeout path verified |
| Conditional GET | ✅ | ✅ | `client_verified` | Single-download CLI path handles `304 Not Modified` with overwrite gate |
| Overwrite existing output safely | ✅ | ✅ | `client_verified` | `--allow-overwrite` truncates stale tail bytes instead of preserving old data |

## FTP/FTPS

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic download | ✅ | ✅ | `wired` | Backend exists; no binary-path E2E yet |
| Passive mode | ✅ | ✅ | `wired` | Provided by suppaftp |
| Range / resume (REST) | ✅ | ✅ | `wired` | Implemented, lifecycle cleanup still needs hardening |
| Explicit FTPS | ✅ | ✅ | `has_code` | Library support available; no dedicated path coverage |
| Implicit FTPS | ✅ | ❌ | `gap` | Deferred |
| FTP proxy | ✅ | ❌ | `has_code` | Not implemented |
| Data stream cleanup | ✅ | 🔧 | `tested` | Wrapper exists, but deeper lifecycle hardening still planned |

## SFTP

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic download | ✅ | ✅ | `wired` | Backend exists; no end-to-end binary test yet |
| Password auth | ✅ | ✅ | `wired` | URL credential path implemented |
| Key auth | ✅ | ✅ | `wired` | Config and backend support added; end-to-end SFTP verification still pending |
| Host key verification | ✅ | ✅ | `wired` | Strict known_hosts policy implemented and unit-tested |
| SFTP proxy | ✅ | ❌ | `has_code` | Not implemented |

## BitTorrent

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic torrent download | ✅ | ✅ | `tested` | `BtService` wired through daemon path and RPC job creation tests |
| Magnet URI | ✅ | ✅ | `tested` | RPC and CLI dispatch paths covered |
| DHT | ✅ | ✅ | `wired` | librqbit support; no explicit parity verification |
| PEX | ✅ | ✅ | `wired` | librqbit support |
| uTP | ✅ | ✅ | `wired` | librqbit support |
| File selection | ✅ | ❌ | `has_code` | Pending capability spike and integration |
| Pause / Resume | ✅ | ✅ | `wired` | Service methods exist; no client verification yet |
| Fastresume | ✅ | ✅ | `wired` | librqbit native behavior |
| MSE/PSE encryption | ✅ | ❌ | `gap` | BT-GAP-001 |
| WebSeed (BEP-17/19) | ✅ | ❌ | `gap` | BT-GAP-002 |
| Rarest-first | ✅ | ❌ | `gap` | BT-GAP-003 |
| HTTP+BT mixed source | ✅ | ❌ | `gap` | BT-GAP-004 |
| SOCKS5 proxy | ✅ | ✅ | `wired` | librqbit supports it; not product-verified |

## Metalink

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Metalink v3 (XML) | ✅ | ✅ | `tested` | Parser coverage exists |
| Metalink v4 (XML) | ✅ | ✅ | `tested` | Parser coverage exists |
| URL priority | ✅ | ✅ | `tested` | Normalizer sorts URLs; runtime selection still simplistic |
| Hash verification | ✅ | ✅ | `wired` | Parser + checksum pieces exist, not yet fully chained |
| Chunk checksum | ✅ | ❌ | `has_code` | Not implemented |
| Multi-mirror failover | ✅ | ❌ | `has_code` | Not implemented |
| Metalink/HTTP (RFC 6249) | ✅ | ❌ | `has_code` | Not implemented |

## Core Engine

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Job lifecycle | ✅ | ✅ | `tested` | Engine unit coverage |
| Concurrent scheduling | ✅ | ✅ | `tested` | Scheduler + executor tests |
| Persistence (crash recovery) | ✅ | ✅ | `tested` | Restore and session smoke cover current behavior |
| Rate limiting | ✅ | ✅ | `tested` | Governor-backed tests |
| Checksum verification | ✅ | ✅ | `tested` | SHA-256 / SHA-1 / MD5 coverage |
| File preallocation | ✅ | ✅ | `tested` | Hot path connected, executor allocation tests added |
| Session save / restore | ✅ | ✅ | `tested` | Current daemon smoke covers graceful save + restore |
| Signal handling (SIGUSR1 etc.) | ✅ | ❌ | `has_code` | Only Ctrl+C path is handled today |
