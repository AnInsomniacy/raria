# raria

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)

`raria` is a modern Rust download manager focused on backend correctness, durable task state, protocol coverage, and a native control model.

The project uses aria2 as a feature reference, not as an API, storage, or configuration compatibility target. The long-term public surface is native raria: `raria.toml`, `/api/v1` JSON resources, `/api/v1/events` WebSocket events, versioned persistence schemas, and CLI names that describe raria concepts directly.

## Current Status

The current tree is a real backend, not a skeleton:

- multi-protocol downloads across HTTP, HTTPS, FTP, FTPS, SFTP, BitTorrent, and Metalink
- segmented range downloads, restart and resume, session persistence, and restore
- native HTTP JSON daemon routes with a WebSocket event stream
- checksum enforcement, conditional GET, mirror failover, and daemon-path Metalink execution
- structured JSON file logging for the highest-value runtime surfaces

The migration is still in progress. Several internal paths and tests continue to use the old JSON-RPC layer as a temporary regression harness while native equivalents are being built.

## Implemented Capabilities

| Area                  | Current implementation                                                                                                                                                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HTTP/HTTPS            | Segmented downloads, resume, redirects, auth, headers, cookies, proxy support, TLS options, conditional GET, checksum verification                                                     |
| FTP/FTPS              | Download, probe, resume, proxy support, explicit FTPS with custom CA                                                                                                                   |
| SFTP                  | Password and key auth, strict known-host verification, proxy support                                                                                                                   |
| BitTorrent            | Magnet and torrent ingestion, file selection intent, metadata projection, tracker override support, peer projection, `Active -> Seeding -> Complete`, one-shot BT completion semantics |
| Metalink              | XML parsing, normalization, mirror priority handling, checksum and piece-checksum wiring, relation projection, daemon-path mirror failover                                             |
| Daemon control plane  | Native `/api/v1` health, config, task, stats, task-control, and event routes, with remaining JSON-RPC code used as a migration harness                                                 |
| Runtime observability | Structured JSON file logs, session correlation, daemon lifecycle events, mirror/source failure events, BT lifecycle events, restore events, native control events, and WS emission logs |
| Persistence           | `redb`-backed job/session persistence and restore across daemon restart                                                                                                                |

## Verified Runtime Behaviors

These behaviors are backed by repository tests and current code:

- daemon restart restores persisted jobs and resumes partial range downloads
- throttled active downloads respond correctly to runtime limit changes
- signal-driven daemon shutdown cancels throttled active downloads promptly
- mirror failover emits non-terminal `SourceFailed` events before eventual completion
- source-failure events are available through the native event model and older migration notification path
- terminal checksum and piece-integrity failures reject invalid output instead of leaving corrupt files behind
- structured log files redact obvious secrets and credential-bearing URLs on covered paths

The durable verification contract lives in [`docs/verification-contract.md`](docs/verification-contract.md).

## Control Surface

The native daemon control surface is an HTTP JSON API under `/api/v1`.

Current native routes include:

- `GET /api/v1/health`
- `GET /api/v1/config`
- `GET /api/v1/stats`
- `GET /api/v1/tasks`
- `POST /api/v1/tasks`
- `GET /api/v1/tasks/{taskId}`
- `DELETE /api/v1/tasks/{taskId}`
- `POST /api/v1/tasks/{taskId}/pause`
- `POST /api/v1/tasks/{taskId}/resume`
- `POST /api/v1/tasks/{taskId}/restart`
- `GET /api/v1/tasks/{taskId}/files`
- `GET /api/v1/tasks/{taskId}/sources`
- `GET /api/v1/events`

The event stream uses stable raria event names such as `task.started`, `task.progress`, `task.paused`, `task.completed`, `task.failed`, `task.removed`, and `task.source.failed`.

## Structured Logging

When `--log <path>` is enabled on the daemon, the file sink is structured JSON rather than mixed human text.

The bounded logging contract currently covers:

- daemon lifecycle and download loop
- core task lifecycle and failure paths
- control-plane mutation and WebSocket event emission

See [`docs/logging-contract.md`](docs/logging-contract.md) for the exact contract and non-goals.

## Quick Start

### Build

```bash
cargo build --release
```

The binary is emitted at `target/release/raria`.

### Single Download

```bash
raria download https://example.com/file.iso -x 16
raria download https://example.com/file.iso -d ~/Downloads -o file.iso
raria download https://example.com/file.iso --checksum sha-256=<hex>
```

### Daemon

```bash
raria daemon -d ~/Downloads --api-port 6800
```

### Native API Example

```bash
curl -X POST http://127.0.0.1:6800/api/v1/tasks \
  -H "Content-Type: application/json" \
  -d '{
    "sources": ["https://example.com/file.iso"],
    "downloadDir": "/tmp",
    "filename": "file.iso",
    "segments": 8
  }'
```

### Native Configuration

```toml
[daemon]
download_dir = "~/Downloads"
session_path = "raria.session.redb"
max_active_tasks = 5

[api]
host = "127.0.0.1"
port = 6800
auth_token_file = "raria.token"

[downloads]
default_segments = 5
min_segment_size = 0
retry_max_attempts = 5
```

## Repository Layout

```text
crates/
  raria-core      engine, job model, scheduler, persistence, checksum, logging helpers
  raria-range     segmented executor and backend contracts
  raria-http      HTTP/HTTPS backend
  raria-ftp       FTP/FTPS backend
  raria-sftp      SFTP backend
  raria-bt        BitTorrent service layer
  raria-metalink  Metalink parser and normalizer
  raria-rpc       native HTTP JSON API, event stream, migration control harness
  raria-cli       CLI and daemon runtime wiring
docs/
  modernization/
  practical-maturity.md
  verification-contract.md
  bt-stop-lines.md
  logging-contract.md
```

## Verification

The minimum full-repository verification bar is:

```bash
cargo test --workspace
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Repository prose should only claim behavior that is either:

- backed by durable test/code anchors, or
- backed by fresh rerun evidence for the current tree

## License

Apache 2.0. See [LICENSE](LICENSE).
