# Protocol Parity Matrix: raria vs aria2 1.37.0

> Updated: 2026-04-09 | Baseline: aria2 1.37.0

## Legend

| Status | Meaning |
|--------|---------|
| ✅ done | Implemented and tested |
| 🔧 partial | Partially implemented, needs work |
| ❌ stub | Stub/placeholder only |
| ⏸️ deferred | Intentionally deferred |
| 🚫 gap | Known incompatibility, will not implement |

---

## HTTP/HTTPS

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic download | ✅ | ✅ | ✅ done | reqwest-based |
| Range requests (segmented) | ✅ | ✅ | ✅ done | ByteSourceBackend trait |
| Resume (partial download) | ✅ | 🔧 | 🔧 partial | Segments not checkpointed yet |
| Content-Disposition filename | ✅ | ❌ | ❌ stub | Not parsed |
| ETag/Last-Modified conditional | ✅ | ❌ | ❌ stub | Field defined but never filled |
| Redirect following | ✅ | ✅ | ✅ done | reqwest default behavior |
| Redirect policy config | ✅ | ❌ | ❌ stub | No --max-redirect |
| HTTP proxy | ✅ | ❌ | ❌ stub | |
| HTTPS proxy | ✅ | ❌ | ❌ stub | |
| SOCKS5 proxy | ✅ | ❌ | ❌ stub | reqwest has feature |
| Cookie file (Netscape) | ✅ | ❌ | ❌ stub | |
| .netrc auth | ✅ | ❌ | ❌ stub | |
| Custom headers | ✅ | 🔧 | 🔧 partial | RpcOptions has header field |
| TLS CA certificate | ✅ | ❌ | ❌ stub | |
| TLS client cert | ✅ | ❌ | ❌ stub | |
| Disable cert verification | ✅ | ❌ | ❌ stub | |
| Basic/Digest auth | ✅ | ❌ | ❌ stub | |
| Metalink/HTTP (RFC 6249) | ✅ | ❌ | ❌ stub | |

## FTP/FTPS

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic download | ✅ | ✅ | ✅ done | suppaftp-based |
| Passive mode | ✅ | ✅ | ✅ done | |
| Range/resume (REST) | ✅ | ✅ | 🔧 partial | Works but uses mem::forget |
| Explicit FTPS | ✅ | 🔧 | 🔧 partial | suppaftp supports it |
| Implicit FTPS | ✅ | ❌ | ⏸️ deferred | |
| FTP proxy | ✅ | ❌ | ❌ stub | |
| Data stream cleanup | ✅ | ❌ | ❌ stub | Currently mem::forget leak |

## SFTP

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic download | ✅ | ✅ | ✅ done | russh + russh-sftp |
| Password auth | ✅ | 🔧 | 🔧 partial | |
| Key auth | ✅ | 🔧 | 🔧 partial | |
| Host key verification | ✅ | ❌ | ❌ stub | |
| SFTP proxy | ✅ | ❌ | ❌ stub | |

## BitTorrent

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Basic torrent download | ✅ | ❌ | ❌ stub | BtService exists but not wired |
| Magnet URI | ✅ | ❌ | ❌ stub | |
| DHT | ✅ | ✅ | 🔧 partial | librqbit supports |
| PEX | ✅ | ✅ | 🔧 partial | librqbit supports |
| uTP | ✅ | ✅ | 🔧 partial | librqbit supports |
| File selection | ✅ | ❌ | ❌ stub | librqbit only_files API |
| Pause/Resume | ✅ | ❌ | ❌ stub | |
| Fastresume | ✅ | ✅ | 🔧 partial | librqbit native |
| MSE/PSE encryption | ✅ | ❌ | 🚫 gap | BT-GAP-001 |
| WebSeed (BEP-17/19) | ✅ | ❌ | 🚫 gap | BT-GAP-002 |
| Rarest-first | ✅ | ❌ | 🚫 gap | BT-GAP-003 |
| HTTP+BT mixed source | ✅ | ❌ | 🚫 gap | BT-GAP-004 |
| SOCKS5 proxy | ✅ | ✅ | 🔧 partial | librqbit supports |

## Metalink

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Metalink v3 (XML) | ✅ | ✅ | ✅ done | quick-xml parser |
| Metalink v4 (XML) | ✅ | ✅ | ✅ done | quick-xml parser |
| URL priority | ✅ | 🔧 | 🔧 partial | Parsed but not used in download |
| Hash verification | ✅ | 🔧 | 🔧 partial | Checksum module exists |
| Chunk checksum | ✅ | ❌ | ❌ stub | |
| Multi-mirror failover | ✅ | ❌ | ❌ stub | |
| Metalink/HTTP (RFC 6249) | ✅ | ❌ | ❌ stub | |

## Core Engine

| Capability | aria2 | raria | Status | Notes |
|-----------|-------|-------|--------|-------|
| Job lifecycle | ✅ | ✅ | ✅ done | |
| Concurrent scheduling | ✅ | ✅ | ✅ done | Semaphore-based |
| Persistence (crash recovery) | ✅ | 🔧 | 🔧 partial | Jobs saved, segments NOT |
| Rate limiting | ✅ | ✅ | ✅ done | governor crate |
| Checksum verification | ✅ | ✅ | ✅ done | SHA-256/SHA-1/MD5 |
| File preallocation | ✅ | ❌ | ❌ stub | |
| Session save/restore | ✅ | ❌ | ❌ stub | |
| Signal handling (SIGUSR1) | ✅ | ❌ | ❌ stub | |
