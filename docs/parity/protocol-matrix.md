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
| Resume (partial download) | ✅ | ✅ | `tested` | Daemon restart resume, range reuse, If-Range, and single-download `--continue` are covered by automated smoke tests |
| Content-Disposition filename | ✅ | ✅ | `client_verified` | Single-download hot path now honors `suggested_filename` |
| ETag / If-Range | ✅ | ✅ | `tested` | Dedicated daemon resume smoke verifies persisted ETag is sent back via If-Range on restart |
| Redirect following | ✅ | ✅ | `tested` | reqwest default behavior |
| Redirect policy config | ✅ | ✅ | `client_verified` | `--max-redirect` verified on the binary path |
| HTTP proxy | ✅ | ✅ | `tested` | Dedicated HTTP proxy smoke verifies absolute-form requests traverse the configured proxy |
| HTTPS proxy | ✅ | ✅ | `tested` | Dedicated CONNECT-proxy smoke verifies HTTPS requests traverse the configured proxy |
| SOCKS5 proxy | ✅ | ✅ | `tested` | Dedicated HTTP config smoke verifies requests traverse a SOCKS5 proxy |
| Cookie file (Netscape) | ✅ | ✅ | `tested` | Hot path verified by integration smoke |
| `.netrc` auth | ✅ | ✅ | `client_verified` | `--netrc-path` verified on the binary path |
| `no-netrc` credential suppression | ✅ | ✅ | `client_verified` | `--no-netrc` verified on the binary path |
| Custom headers | ✅ | ✅ | `client_verified` | RPC path covered and CLI binary path verified |
| TLS CA certificate | ✅ | ✅ | `tested` | Dedicated mTLS smoke verifies custom CA trust + client certificate path |
| Disable cert verification | ✅ | ✅ | `tested` | Dedicated HTTPS smoke verifies self-signed TLS works when certificate verification is disabled |
| Basic auth | ✅ | ✅ | `client_verified` | Verified on both single-download CLI and daemon/RPC paths |
| Digest auth | ✅ | ✅ | `tested` | Dedicated HTTP config smoke verifies 401 Digest challenge → authenticated retry |
| Metalink/HTTP (RFC 6249) | ✅ | ❌ | `gap` | Accepted gap: dynamic HTTP-header-based Metalink discovery is outside the current replacement scope |
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
| Explicit FTPS | ✅ | ✅ | `wired` | `ftps://` now upgrades via AUTH TLS and PROT/PBSZ on the real FTP hot path; dedicated E2E coverage still pending |
| Implicit FTPS | ✅ | ❌ | `gap` | Deferred |
| FTP proxy | ✅ | ✅ | `wired` | FTP hot path now consumes `all_proxy=socks5://...`; dedicated FTP proxy E2E is still pending |
| Data stream cleanup | ✅ | 🔧 | `tested` | Wrapper exists, but deeper lifecycle hardening still planned |

## SFTP

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic download | ✅ | ✅ | `wired` | Backend exists; no end-to-end binary test yet |
| Password auth | ✅ | ✅ | `wired` | URL credential path implemented |
| Key auth | ✅ | ✅ | `wired` | Config and backend support added; end-to-end SFTP verification still pending |
| Host key verification | ✅ | ✅ | `wired` | Strict known_hosts policy implemented and unit-tested |
| SFTP proxy | ✅ | ✅ | `wired` | SFTP hot path now consumes `all_proxy=socks5://...` via proxied TCP stream; dedicated E2E is still pending |

## BitTorrent

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic torrent download | ✅ | ✅ | `tested` | `BtService` wired through daemon path and RPC job creation tests |
| Magnet URI | ✅ | ✅ | `tested` | RPC and CLI dispatch paths covered |
| DHT | ✅ | ✅ | `wired` | librqbit support; no explicit parity verification |
| PEX | ✅ | ✅ | `wired` | librqbit support |
| uTP | ✅ | ✅ | `wired` | librqbit support |
| File selection | ✅ | ✅ | `tested` | BT selection is wired and covered by unit + RPC tests |
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
| URL priority | ✅ | ✅ | `tested` | Metalink add path now normalizes and persists mirror order by priority/preference |
| Hash verification | ✅ | ✅ | `tested` | `addMetalink` now persists preferred file checksum into job options for product-path verification |
| Chunk checksum | ✅ | ❌ | `gap` | Accepted gap: piece-level Metalink checksum enforcement needs deeper parser + scheduler integration than the current product scope |
| Multi-mirror failover | ✅ | ✅ | `tested` | Daemon range hot path now fails over to the next mirror when an earlier mirror fails |
| Metalink/HTTP (RFC 6249) | ✅ | ❌ | `gap` | Accepted gap: dynamic HTTP-header-based Metalink discovery is outside the current replacement scope |

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
| Signal handling (SIGUSR1 etc.) | ✅ | ✅ | `tested` | SIGUSR1 session save + SIGTERM graceful shutdown in daemon |
