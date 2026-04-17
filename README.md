# raria

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)

`raria` is a Rust download engine focused on backend correctness, honest capability projection, and practical aria2-style interoperability.

It is not a nostalgia project and it is not trying to reproduce every historical aria2 quirk. The target is a production-grade Rust downloader with a usable aria2-style JSON-RPC and WebSocket control surface, explicit stop-lines where parity is not worth forcing, and repository documentation that matches the code.

## Current Status

The current tree is a real backend, not a skeleton:

- multi-protocol downloads across HTTP, HTTPS, FTP, FTPS, SFTP, BitTorrent, and Metalink
- segmented range downloads, restart and resume, session persistence, and restore
- aria2-style JSON-RPC daemon behavior with WebSocket notifications
- checksum enforcement, conditional GET, mirror failover, and daemon-path Metalink execution
- structured JSON file logging for the highest-value runtime surfaces

The repository uses explicit stop-lines instead of pretending unsupported parity exists.

## Implemented Capabilities

| Area                  | Current implementation                                                                                                                                                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HTTP/HTTPS            | Segmented downloads, resume, redirects, auth, headers, cookies, proxy support, TLS options, conditional GET, checksum verification                                                     |
| FTP/FTPS              | Download, probe, resume, proxy support, explicit FTPS with custom CA                                                                                                                   |
| SFTP                  | Password and key auth, strict known-host verification, proxy support                                                                                                                   |
| BitTorrent            | Magnet and torrent ingestion, file selection intent, metadata projection, tracker override support, peer projection, `Active -> Seeding -> Complete`, one-shot BT completion semantics |
| Metalink              | XML parsing, normalization, mirror priority handling, checksum and piece-checksum wiring, relation projection, daemon-path mirror failover                                             |
| Daemon control plane  | aria2-style JSON-RPC methods, multicall, session info, global stat, option mutation, health endpoint, WebSocket notifications                                                          |
| Runtime observability | Structured JSON file logs, session correlation, daemon lifecycle events, mirror/source failure events, BT lifecycle events, restore events, RPC control and WS emission logs           |
| Persistence           | `redb`-backed job/session persistence and restore across daemon restart                                                                                                                |

## Verified Runtime Behaviors

These behaviors are backed by repository tests and current code:

- daemon restart restores persisted jobs and resumes partial range downloads
- throttled active downloads respond correctly to runtime limit changes
- signal-driven daemon shutdown cancels throttled active downloads promptly
- mirror failover emits non-terminal `SourceFailed` events before eventual completion
- `SourceFailed` is available through aria2-style WebSocket notifications as `aria2.onSourceFailed`
- terminal checksum and piece-integrity failures reject invalid output instead of leaving corrupt files behind
- structured log files redact obvious secrets and credential-bearing URLs on covered paths

The durable verification contract lives in [`docs/verification-contract.md`](docs/verification-contract.md).

## Control Surface

`raria` provides an aria2-style JSON-RPC and WebSocket control plane intended for downstream clients that already understand the aria2 model.

Current surface highlights:

- `aria2.addUri`
- `aria2.addTorrent`
- `aria2.addMetalink`
- `aria2.tellStatus`, `aria2.tellActive`, `aria2.tellWaiting`, `aria2.tellStopped`
- `aria2.getFiles`, `aria2.getUris`, `aria2.getPeers`, `aria2.getServers`
- `aria2.getOption`, `aria2.changeOption`
- `aria2.getGlobalOption`, `aria2.changeGlobalOption`
- `aria2.pause`, `aria2.unpause`, `aria2.pauseAll`, `aria2.unpauseAll`
- `aria2.remove`, `aria2.forceRemove`, `aria2.removeDownloadResult`, `aria2.purgeDownloadResult`
- `aria2.shutdown`, `aria2.saveSession`, `aria2.getVersion`, `aria2.getSessionInfo`
- `system.multicall`, `system.listMethods`, `system.listNotifications`

Current WebSocket notification coverage includes:

- `aria2.onDownloadStart`
- `aria2.onDownloadPause`
- `aria2.onDownloadStop`
- `aria2.onDownloadComplete`
- `aria2.onDownloadError`
- `aria2.onSourceFailed`
- `aria2.onBtDownloadComplete`

## Structured Logging

When `--log <path>` is enabled on the daemon, the file sink is structured JSON rather than mixed human text.

The bounded logging contract currently covers:

- daemon lifecycle and download loop
- core task lifecycle and failure paths
- RPC mutation and WebSocket notification emission

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
raria daemon -d ~/Downloads --rpc-port 6800
```

### JSON-RPC Example

```bash
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": "1",
    "method": "aria2.addUri",
    "params": [["https://example.com/file.iso"], {"dir": "/tmp"}]
  }'
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
  raria-rpc       aria2-style JSON-RPC, events, server, facade
  raria-cli       CLI and daemon runtime wiring
docs/
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
