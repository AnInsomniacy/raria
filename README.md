# raria

`raria` is a Rust-native download engine built as a clean new-session replacement for aria2-style CLI and JSON-RPC workflows.

The project is a rewrite, not a port. It keeps the integration surfaces that matter for new tasks, uses mature Rust libraries for protocol work, and removes legacy compatibility layers that would make the new core harder to reason about.

## Status

This repository currently contains a phase-one implementation.

Supported new-session paths include direct CLI downloads, aria2-style input files, save-session text for new raria tasks, JSON-RPC over HTTP POST, WebSocket notifications, HTTP(S), FTP, SFTP, BitTorrent torrent and magnet tasks, Metalink v3/v4 ingestion, selected BitTorrent file downloads, and `.raria` control files for new-task resume.

The current build is intended for clean raria sessions. It does not read old `.aria2` control files, old aria2 sessions, old queues, old partial task state, old caches, or old history.

## Design Goals

`raria` prioritizes low-friction replacement for applications that already start aria2-like workers, submit new downloads, poll JSON-RPC status, pause or remove tasks, read global stats, and listen for WebSocket events.

Internally, the codebase favors modern async Rust and library-backed protocol adapters over aria2's older C++ architecture. Protocol parsing, transport, hashing, rate limiting, XML, bencode, FTP, SFTP, HTTP, and BitTorrent behavior should come from maintained crates when a suitable crate exists. Hand-written code is limited to raria-specific task orchestration, compatibility shaping, `.raria` state, focused adapters, and small domain mappings.

## Implemented Capabilities

| Area | Current support |
| --- | --- |
| CLI | Common aria2-style options, direct URI execution, `--input-file`, `--save-session`, `--dir`, `--split`, `--continue`, `--enable-rpc`, `--rpc-listen-port`, and `--rpc-secret` parsing. |
| JSON-RPC | HTTP POST and WebSocket transport on `/jsonrpc`, aria2-shaped method names, token parameter stripping, `system.multicall`, method listing, notification listing, polling, queue status, pause, unpause, remove, global stats, version, session info, and save-session acknowledgement. |
| HTTP(S) | Downloads through `reqwest`, range resume, split range requests, checksums, headers, cookies, netrc credentials, HTTP proxy, and per-task download limits. |
| FTP | Basic downloads, username/password options, and `.raria` resume through `suppaftp`. |
| SFTP | Basic downloads, username/password options, and `.raria` resume through `russh` and `russh-sftp`. |
| BitTorrent | Torrent bytes and magnet URIs through `librqbit`, DHT for public magnet tasks, initial-peer fixtures, file selection mapping, metadata status, and completion waits. |
| Metalink | Metalink v3/v4 XML parsing through `quick-xml`, HTTP resource extraction, file size mapping, and SHA-256 checksum mapping. |
| Persistence | Versioned `.raria` JSON control files with atomic writes for new-task resume. |

## Explicit Non-Goals

`raria` does not implement ED2K transfer or ED2K search in phase one. ED2K-related RPC methods return explicit unsupported errors.

`raria` also does not implement old `.aria2` state migration, XML-RPC, JSONP, JSON-RPC GET, deprecated `rpc-user` and `rpc-passwd`, HTTP pipelining, backend event-poll selection, built-in daemonization, process hook commands, or libaria2 C API compatibility.

BitTorrent seeding is not a long-running mode yet. The current CLI path waits for download completion, stops the `librqbit` session, and exits.

## Quick Start

Build the release binary:

```bash
cargo build --release
```

The binary is emitted at `target/release/raria`.

Download one URI:

```bash
target/release/raria --dir ~/Downloads https://example.com/file.iso
```

Download from an aria2-style input file:

```text
https://example.com/a.iso
  out=a.iso
  split=4

magnet:?xt=urn:btih:...
  select-file=2-8
```

```bash
target/release/raria --dir ~/Downloads --input-file tasks.txt --save-session session.txt
```

Start the foreground JSON-RPC server:

```bash
target/release/raria --enable-rpc=true --rpc-listen-port=6800 --dir ~/Downloads
```

Submit a download through JSON-RPC:

```bash
curl -s http://127.0.0.1:6800/jsonrpc \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": "add",
    "method": "aria2.addUri",
    "params": [["https://example.com/file.iso"], {"split": "4"}]
  }'
```

Poll status:

```bash
curl -s http://127.0.0.1:6800/jsonrpc \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": "status",
    "method": "aria2.tellStatus",
    "params": ["0000000000000001"]
  }'
```

## RPC Surface

The phase-one RPC surface centers on new tasks and status polling. Implemented or explicitly shaped methods include `aria2.addUri`, `aria2.addTorrent`, `aria2.addMetalink`, `aria2.tellStatus`, `aria2.getFiles`, `aria2.getUris`, `aria2.tellActive`, `aria2.tellWaiting`, `aria2.tellStopped`, `aria2.pause`, `aria2.forcePause`, `aria2.unpause`, `aria2.remove`, `aria2.forceRemove`, `aria2.getGlobalStat`, `aria2.getVersion`, `aria2.getSessionInfo`, `aria2.saveSession`, `system.multicall`, `system.listMethods`, and `system.listNotifications`.

WebSocket clients connect to `/jsonrpc` and receive aria2-shaped notifications such as `aria2.onDownloadStart`, `aria2.onDownloadPause`, `aria2.onDownloadStop`, `aria2.onDownloadComplete`, `aria2.onDownloadError`, and `aria2.onBtDownloadComplete`.

Unsupported legacy or later-phase methods are kept discoverable where useful and return explicit errors instead of pretending to work.

## File State

Partial new-task state uses `.raria` files. The format is versioned JSON and intentionally separate from aria2's `.aria2` format.

This separation is deliberate. It keeps the new implementation free to use a cleaner state model while making migration expectations visible: start new tasks with raria, do not point it at old aria2 state and expect automatic recovery.

## Repository Layout

```text
crates/
  raria-core      core engine, CLI parsing, RPC model, protocol adapters, persistence
  raria-cli       binary entry point and runtime wiring
docs/
  raria-goal/     implementation memory, contracts, decisions, checkpoints, verification ledger
```

## Verification

The normal local verification bar is:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

Recent end-to-end evidence for the current tree is kept outside the repository under `~/Desktop/raria-end-to-end-test`. The latest public magnet smoke test used `select-file=2-8` against the KNOPPIX magnet, skipped the largest ISO payload, downloaded the remaining small files, and completed in 9.18 seconds.

## License

The Cargo metadata declares `MIT OR Apache-2.0`. License text files have not been added to this repository yet.
