# Source Audit Baseline

This document records the source-driven audit inputs for the raria modernization goal. It does not claim the audit is complete. It establishes the first reproducible checkpoint.

## raria Audit Inputs

Workspace: `/Users/sekiro/Projects/personal/raria`

Branch: `main`

Included inputs:

- Workspace manifest and crate manifests.
- All `crates/*/src/*.rs` files.
- All `crates/*/tests/*.rs` files.
- Repository Markdown documentation.
- Rust toolchain and formatting configuration.

Excluded inputs:

- `.git`
- `target`
- Dependency source code.
- Generated build output.

Observed raria structure:

- `raria-core`: job model, scheduler, registry, persistence, configuration, checksum, progress events, cancellation, speed and limiter utilities.
- `raria-range`: byte-source backend contract and segmented executor.
- `raria-http`: HTTP/HTTPS backend, cookies, content disposition, authentication, proxy and TLS configuration.
- `raria-ftp`: FTP/FTPS backend.
- `raria-sftp`: SFTP backend.
- `raria-metalink`: Metalink parser and normalizer.
- `raria-bt`: librqbit-backed BitTorrent service, torrent metadata parsing, WebSeed pre-download support.
- `raria-rpc`: aria2-style JSON-RPC and WebSocket control surface.
- `raria-cli`: single-download path, daemon runtime, BT runtime wiring, hooks, backend factory.

Current high-confidence raria coverage:

- HTTP/HTTPS, FTP/FTPS, SFTP byte-source downloads.
- Segmented range execution with retry, cancellation, checkpointing, file allocation, progress callbacks, and per-task limiting.
- Metalink parsing and normalization.
- BitTorrent ingestion through librqbit for magnet and torrent inputs.
- DHT persistence wiring around librqbit.
- WebSeed pre-download support for torrent file inputs.
- redb-backed job and segment persistence.
- aria2-style JSON-RPC and WebSocket notifications.
- CLI daemon and single-download entry points.

Current modernization conflicts:

- Public control is aria2-style JSON-RPC instead of raria-native HTTP JSON API and event schema.
- Configuration parsing is aria2-style key-value config instead of `raria.toml`.
- Job identifiers and response projection are intentionally aria2-shaped in several modules.
- Tests and docs still describe parity and aria2 compatibility as target behavior.
- Persistence stores serialized current structs without a full versioned native schema and migration plan.

## aria2 Audit Inputs

Reference tree: `/Users/sekiro/Projects/oss/aria2`

Included inputs:

- `src/*` aria2 source files.
- `test/*` aria2 tests and test fixtures.
- `doc/manual-src/en/aria2c.rst`
- `doc/manual-src/en/technical-notes.rst`
- `doc/manual-src/en/libaria2.rst` only as an excluded legacy API reference.
- Top-level README and release notes where useful for feature discovery.

Excluded inputs:

- `.git`
- build output.
- dependency directories, including `deps`.
- packaging-only platform material unless it indicates a downloader capability.

Manual-derived inventory:

- 197 documented CLI options were extracted from `doc/manual-src/en/aria2c.rst`.
- 35 documented RPC and notification names were extracted from `doc/manual-src/en/aria2c.rst`.
- Manual sections used for feature grouping include Basic, HTTP/FTP/SFTP, HTTP-specific, FTP/SFTP-specific, BitTorrent/Metalink, BitTorrent-specific, Metalink-specific, RPC, Advanced, option notes, RPC interface, WebSocket, examples, Metalink, BitTorrent, and BitTorrent encryption.

Source-derived feature anchors:

- `OptionHandlerFactory.cc`, `prefs.h`, and `option_processing.cc` define option semantics and config behavior.
- `download_helper.cc`, `ProtocolDetector`, and request-group construction define input classification and task creation.
- `RequestGroupMan`, `RequestGroup`, `DownloadContext`, `FileEntry`, and `SegmentMan` define queue, file, segment, and lifecycle behavior.
- `DefaultPieceStorage`, `Piece`, `PieceStatMan`, and piece selectors define piece scheduling and verification surfaces.
- `Http*`, `Ftp*`, and `Sftp*` source files define protocol behavior.
- `Metalink*` source files define Metalink parsing, filtering, mirror ordering, checksum, piece checksum, and metaurl behavior.
- `Bt*`, `DHT*`, `UDPTracker*`, `UTMetadata*`, and `UTPex*` source files define the modern BitTorrent capability set.
- `RpcMethod*`, `Json*`, `WebSocket*`, and `HttpServer*` source files define the legacy control plane that is used only as a feature reference.

Explicitly excluded legacy or non-target surfaces:

- XML-RPC.
- libaria2 C API compatibility.
- aria2 legacy session/control-file compatibility.
- HTTP/1.1 pipelining.
- BitTorrent MSE/ARC4 encryption.
- LPD.
- aria2 config syntax compatibility.
- Historical packaging and platform-specific compatibility baggage.

