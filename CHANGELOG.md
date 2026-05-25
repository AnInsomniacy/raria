# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-04-09

### Changed

- Product documentation now describes the native raria contract only.
- The daemon control plane is the `/api/v1` HTTP JSON API with `/api/v1/events` WebSocket events.
- The CLI exposes native shell completion through `raria completion <shell>`.

### Removed

- Removed old external API surfaces, old method names, old option names, and old client bridge expectations from the product contract.

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
- FTP/FTPS backend via suppaftp
- SFTP backend via russh and russh-sftp
- BitTorrent service integration via librqbit

**Metalink**
- Metalink 4 XML parser (quick-xml)
- Normalizer for URL priority sorting and hash selection

**Checksum**
- SHA-256 file hashing
- Checksum spec parser (`algo=hex` format)
- Post-download verification

**Native API**
- HTTP JSON API and WebSocket event stream via axum
- Native task, stats, session, daemon shutdown, transfer, file, source, tracker, peer, and seeding resources
- Native event envelope for lifecycle, progress, source failure, BitTorrent metadata, seeding, peer, and tracker updates

**CLI**
- `raria download <URL>` — single-shot download with progress output
- `raria daemon` — persistent process with native API server and scheduler loop
- `raria completion <shell>` — generated native shell completion
- Ctrl+C graceful shutdown via engine cancel registry
- native download, daemon, transfer, proxy, auth, session, and integrity flags

### Testing
- Focused unit, integration, contract, and smoke tests across all crates
- Final release validation requires `cargo fmt --all --check`, `cargo check --workspace --locked`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`
