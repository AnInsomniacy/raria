# raria

A high-performance, multi-protocol download utility written in Rust. Designed as a modern, library-first alternative to [aria2](https://aria2.github.io/), with an aria2-compatible JSON-RPC interface.

## Features

- **Multi-connection downloads** — split files into segments and download them concurrently
- **HTTP/HTTPS** — powered by [reqwest](https://docs.rs/reqwest) with gzip, brotli, cookies, and SOCKS proxy support
- **FTP/FTPS** — via [suppaftp](https://docs.rs/suppaftp) (planned)
- **SFTP** — via [russh](https://docs.rs/russh) (planned)
- **BitTorrent** — via [librqbit](https://docs.rs/librqbit) with DHT, PEX, and magnet link support (planned)
- **Metalink** — v3/v4 XML parser for multi-source downloads
- **aria2-compatible JSON-RPC** — drop-in replacement for aria2's RPC interface
- **Daemon mode** — persistent background process with session persistence
- **Rate limiting** — global download speed throttle via [governor](https://docs.rs/governor)
- **Checksum verification** — SHA-256 integrity checking after download
- **Crash recovery** — jobs persisted to [redb](https://docs.rs/redb); active downloads resume as waiting after restart

## Quick Start

### Single-file download

```bash
# Download with 16 connections
raria download https://example.com/large-file.zip -x 16

# Custom output directory and filename
raria download https://example.com/file.zip -d /tmp/downloads -o myfile.zip

# With speed limit (bytes/sec)
raria download https://example.com/file.zip --max-download-limit 1048576

# With checksum verification
raria download https://example.com/file.zip --checksum sha-256=abc123...
```

### Daemon mode (with RPC)

```bash
# Start daemon on port 6800
raria daemon -d /tmp/downloads --rpc-port 6800

# Submit a download via JSON-RPC
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "aria2.addUri",
    "id": "1",
    "params": [["https://example.com/file.zip"], {"dir": "/tmp/downloads"}]
  }'

# Check status
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"aria2.tellStatus","id":"2","params":["0000000000000001"]}'

# Get global statistics
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

raria is a Cargo workspace with 9 crates:

```
raria/
├── crates/
│   ├── raria-core      # Job model, engine, scheduler, persistence, config
│   ├── raria-range     # ByteSourceBackend trait + concurrent SegmentExecutor
│   ├── raria-http      # HTTP/HTTPS backend (reqwest)
│   ├── raria-ftp       # FTP/FTPS backend (suppaftp) [stub]
│   ├── raria-sftp      # SFTP backend (russh) [stub]
│   ├── raria-bt        # BitTorrent service (librqbit) [stub]
│   ├── raria-metalink  # Metalink v3/v4 XML parser + normalizer
│   ├── raria-rpc       # aria2-compatible JSON-RPC server (jsonrpsee)
│   └── raria-cli       # CLI binary (clap) — download + daemon modes
├── Cargo.toml          # Workspace manifest
├── LICENSE             # Apache 2.0
└── README.md
```

### Design Principles

1. **Library-first** — all logic lives in library crates; the CLI is a thin integration layer.
2. **Protocol separation** — HTTP/FTP/SFTP implement `ByteSourceBackend`; BT uses a separate `BtService` (session-based, not range-based).
3. **Persistence-by-default** — all job state changes are written to redb before in-memory state is updated.
4. **Cancel propagation** — every activated job gets a `CancellationToken` from the engine's `CancelRegistry`; pause/Ctrl+C propagates automatically.

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
# Run all tests (205 tests)
cargo test --workspace

# Run with logging
RUST_LOG=debug cargo test --workspace

# Clippy lint check
cargo clippy --workspace -- -D warnings
```

## Roadmap

- [x] Core engine with job lifecycle management
- [x] Concurrent segment executor with retry and backoff
- [x] HTTP/HTTPS backend with range support
- [x] Metalink v3/v4 parsing
- [x] redb persistence layer
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
- [ ] Resume/checkpoint support (segment state persistence)

## License

Apache 2.0 — see [LICENSE](LICENSE).
