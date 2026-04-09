# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-04-09

### Added

**Core Infrastructure**
- Job model with state machine (Waiting → Active → Complete/Error/Paused/Removed)
- 16-digit hex GID generation with serde support
- Segment planner for splitting files into parallel byte ranges
- `JobRegistry` — thread-safe in-memory job index
- `Scheduler` — FIFO queue with configurable concurrency limit
- `CancelRegistry` — per-job cancellation token management
- `EventBus` — tokio broadcast channel for progress/status events
- `GlobalConfig` and `JobOptions` with serde support
- `RateLimiter` — governor-based throughput throttle

**Persistence (redb)**
- `Store` with 4 tables: jobs, segments, job_options, global_state
- Full CRUD operations for all tables
- Engine ↔ Store integration: all lifecycle changes persist automatically
- Crash recovery: active jobs demoted to waiting on restore

**Download Engine**
- `ByteSourceBackend` trait for protocol-agnostic range downloads
- `SegmentExecutor` — concurrent multi-connection downloads with:
  - `tokio::spawn` per segment + `Semaphore` for connection limiting
  - Automatic retry with exponential backoff
  - Streaming support for unknown-size files (EOF detection)
  - Optional rate limiter integration
- `Engine` orchestrator with full lifecycle management
  - `add_uri`, `activate_job`, `pause`, `unpause`, `complete_job`, `fail_job`, `remove`
  - CancellationToken returned from `activate_job` for executor control
  - Session restore from persistent store

**Protocols**
- HTTP/HTTPS backend via reqwest (probe + range download)
- FTP/SFTP/BT — trait stubs with type definitions (not yet implemented)

**Metalink**
- v3/v4 XML parser (quick-xml)
- Normalizer for URL priority sorting and hash selection

**Checksum**
- SHA-256 file hashing
- Checksum spec parser (`algo=hex` format)
- Post-download verification

**RPC (aria2-compatible)**
- JSON-RPC server via jsonrpsee (HTTP + WebSocket)
- 10 methods: `addUri`, `tellStatus`, `pause`, `unpause`, `remove`, `getGlobalStat`, `tellActive`, `tellWaiting`, `tellStopped`, `getVersion`
- aria2-compatible response format (camelCase, string-typed numbers)
- Event notification mapping (DownloadEvent → aria2 notifications)
- RPC facade for internal-to-aria2 format translation

**CLI**
- `raria download <URL>` — single-shot download with progress output
- `raria daemon` — persistent process with RPC server and scheduler loop
- Ctrl+C graceful shutdown via engine cancel registry
- `--max-download-limit`, `--checksum`, `--connections` flags

### Testing
- 205 unit and integration tests across all crates
- 0 clippy warnings with `-D warnings`
