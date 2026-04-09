# raria

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-205%20passing-brightgreen.svg)](#testing)

A high-performance, multi-protocol download engine written in Rust. Designed as a native replacement for [aria2](https://aria2.github.io/) — the download engine powering [Motrix Next](https://github.com/AnInsomniacy/motrix-next).

## Features

| Feature | Status | Backend |
|---------|--------|---------|
| Multi-connection HTTP/HTTPS | ✅ Working | [reqwest](https://docs.rs/reqwest) |
| Concurrent segment downloads | ✅ Working | tokio + Semaphore |
| Automatic retry + exponential backoff | ✅ Working | — |
| Session persistence / crash recovery | ✅ Working | [redb](https://docs.rs/redb) |
| aria2-compatible JSON-RPC (10 methods) | ✅ Working | [jsonrpsee](https://docs.rs/jsonrpsee) |
| Daemon mode with scheduler | ✅ Working | tokio |
| Rate limiting | ✅ Working | [governor](https://docs.rs/governor) |
| SHA-256 checksum verification | ✅ Working | [sha2](https://docs.rs/sha2) |
| Metalink v3/v4 parsing | ✅ Working | [quick-xml](https://docs.rs/quick-xml) |
| FTP/FTPS | 🔧 Planned | [suppaftp](https://docs.rs/suppaftp) |
| SFTP | 🔧 Planned | [russh](https://docs.rs/russh) |
| BitTorrent (DHT, PEX, magnet) | 🔧 Planned | [librqbit](https://docs.rs/librqbit) |

## Quick Start

### Single-file download

```bash
# Download with 16 concurrent connections
raria download https://example.com/large-file.zip -x 16

# Custom output directory and filename
raria download https://example.com/file.zip -d ~/Downloads -o myfile.zip

# With speed limit (1 MB/s)
raria download https://example.com/file.zip --max-download-limit 1048576

# With checksum verification
raria download https://example.com/file.zip --checksum sha-256=e3b0c44298fc...
```

### Daemon mode (aria2-compatible RPC)

```bash
# Start daemon with RPC on port 6800
raria daemon -d ~/Downloads --rpc-port 6800

# Add a download via JSON-RPC (same as aria2!)
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "aria2.addUri",
    "id": "1",
    "params": [["https://example.com/file.zip"], {"dir": "/tmp"}]
  }'

# Query status
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"aria2.tellStatus","id":"2","params":["0000000000000001"]}'

# Global stats
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"aria2.getGlobalStat","id":"3","params":[]}'
```

## Building from Source

```bash
# Requirements: Rust 1.85+ (stable)
git clone https://github.com/AnInsomniacy/raria.git
cd raria
cargo build --release
```

The binary is at `target/release/raria`.

## Architecture

raria is a Cargo workspace with 9 crates, designed for library-first usage:

```
raria/
├── crates/
│   ├── raria-core      # Job model, engine, scheduler, persistence, config, checksum
│   ├── raria-range     # ByteSourceBackend trait + concurrent SegmentExecutor
│   ├── raria-http      # HTTP/HTTPS backend (reqwest)
│   ├── raria-ftp       # FTP/FTPS backend (suppaftp) [planned]
│   ├── raria-sftp      # SFTP backend (russh) [planned]
│   ├── raria-bt        # BitTorrent service (librqbit) [planned]
│   ├── raria-metalink  # Metalink v3/v4 XML parser
│   ├── raria-rpc       # aria2-compatible JSON-RPC server (jsonrpsee)
│   └── raria-cli       # CLI binary (clap) — download + daemon modes
├── Cargo.toml          # Workspace manifest
├── LICENSE             # Apache 2.0
└── README.md
```

### Design Principles

1. **Library-first** — all logic lives in library crates; the CLI is a thin integration layer. This enables direct Tauri integration without a sidecar process.
2. **Protocol separation** — HTTP/FTP/SFTP implement `ByteSourceBackend` (range-based downloads); BT uses a separate `BtService` (session-based, piece-driven).
3. **Persistence-by-default** — all job state changes are written to redb before in-memory state is updated. Active jobs are demoted to Waiting on crash recovery.
4. **Cancel propagation** — every activated job gets a `CancellationToken` from the engine's `CancelRegistry`; pause/Ctrl+C/shutdown propagates automatically to all segment tasks.
5. **aria2 compatibility** — RPC responses use aria2's camelCase format with string-typed numbers, so existing aria2 clients (including Motrix Next) work without changes.

## RPC Methods

| Method | Description | Status |
|--------|-------------|--------|
| `aria2.addUri` | Add a new download | ✅ |
| `aria2.tellStatus` | Query job status | ✅ |
| `aria2.pause` | Pause a download | ✅ |
| `aria2.unpause` | Resume a paused download | ✅ |
| `aria2.remove` | Remove a download | ✅ |
| `aria2.getGlobalStat` | Global statistics | ✅ |
| `aria2.tellActive` | List active downloads | ✅ |
| `aria2.tellWaiting` | List waiting downloads | ✅ |
| `aria2.tellStopped` | List stopped downloads | ✅ |
| `aria2.getVersion` | Version information | ✅ |

## Testing

```bash
# Run all tests (205 tests across 9 crates)
cargo test --workspace

# Run with logging
RUST_LOG=debug cargo test --workspace

# Clippy lint check (zero warnings enforced)
cargo clippy --workspace -- -D warnings
```

## Roadmap

- [x] Core engine with job lifecycle state machine
- [x] Concurrent segment executor with retry and backoff
- [x] HTTP/HTTPS backend with range support
- [x] Metalink v3/v4 parsing
- [x] redb persistence layer with crash recovery
- [x] Engine ↔ Store integration
- [x] CancelToken propagation (Engine → Executor)
- [x] Rate limiting integration
- [x] SHA-256 checksum verification
- [x] aria2-compatible JSON-RPC server (10 methods)
- [x] Daemon mode with scheduler run loop
- [ ] FTP/FTPS backend implementation
- [ ] SFTP backend implementation
- [ ] BitTorrent integration via librqbit
- [ ] WebSocket event push notifications
- [ ] Tauri plugin for direct Motrix Next integration
- [ ] Resume/checkpoint support

## Related Projects

- [Motrix Next](https://github.com/AnInsomniacy/motrix-next) — full-featured download manager built with Tauri + Vue 3
- [aria2-builder](https://github.com/AnInsomniacy/aria2-builder) — cross-platform static aria2 builds (the current engine)
- [aria2](https://github.com/aria2/aria2) — the original C++ download utility

## Author

**AnInsomniacy** — [@AnInsomniacy](https://github.com/AnInsomniacy)

## License

Apache 2.0 — see [LICENSE](LICENSE).
