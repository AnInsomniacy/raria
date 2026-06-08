# Progress

## 2026-06-08

Created the tracking structure for the long-running raria goal. The current project position is a new Rust project, not a legacy aria2 state migration. Phase one targets low-friction new-session replacement: JSON-RPC polling, WebSocket events, common CLI/config/input-file flows, HTTP(S), FTP, SFTP, BitTorrent, Metalink, save-session for new tasks, and `.raria` persistence. ED2K is deferred and must return explicit unsupported behavior.

Current checkpoint: `00-discovery`.

Next action: start the goal, then fill the discovery checkpoint from `aria2-next` sources and lock the first contract ledger pass before writing Rust source.

Updated `.gitignore` so normal build, test, and runtime download artifacts stay untracked while future `.raria` fixtures can be committed under test fixture directories.

Discovery pass now records the main public contract shape from `aria2-next`: JSON-RPC over `/jsonrpc`, WebSocket notifications, 38 RPC methods, 6 notification names, `rpc-secret` token parameters, `.aria2` legacy state pruning, and `.raria` as the new control-file format. XML-RPC, JSONP/GET RPC, deprecated `rpc-user`/`rpc-passwd`, HTTP pipelining, event-poll selection, old state migration, and the old C embedding API are out of phase one.

Library audit was refreshed with current crates.io evidence. Tokio, reqwest, quick-xml, governor, clap, serde, tracing, and RustCrypto remain stable-first choices. `jsonrpsee`, `suppaftp`, `russh-sftp`, and `serde + ciborium` need small probes before acceptance. `librqbit` is only a probe candidate because the published crate is currently an rc release, so BitTorrent must not be locked to it until capability and stability risks are recorded.

Completed checkpoint `00-discovery`. `contracts/options.toml` now splits option handling into keep, probe, prune, and unsupported phase-one groups so CLI/config/RPC option behavior can be implemented without silent fake compatibility.

Completed checkpoint `10-foundation`. The Rust workspace now has `raria-core` and the `raria` binary crate, a shared `Result` and `Error` boundary, `RariaConfig` defaults for the new-session contract, and a Tokio runtime shell that starts and shuts down without downloads. Verification passed with `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and goal document CSV/JSON/TOML parsing.

Current checkpoint: `20-cli-config`.

Next action: implement CLI/config/input parsing with focused fixtures for supported, pruned, and unsupported option classes.

Completed checkpoint `20-cli-config` with a focused first parser pass. `raria-core` now exposes `parse_cli`, `parse_config_text`, `parse_input_file_text`, and `save_session_text`. The covered contract includes common new-session CLI options, aria-style config lines, input-file tasks with per-task options, loadable save-session text for new tasks, and explicit dispositions for pruned and phase-one unsupported options. This is intentionally a parser and contract layer only; it does not start downloads.

Current checkpoint: `30-rpc-events`.

Next action: build the JSON-RPC shape and in-process queue state first, then expose HTTP and WebSocket transports once contract fixtures pass.

Completed checkpoint `30-rpc-events`. The core now has an in-memory `RpcEngine`, aria2-shaped GIDs, `aria2.addUri`, `aria2.tellStatus`, pause/unpause/remove, `aria2.getGlobalStat`, `system.multicall`, `system.listMethods`, `system.listNotifications`, explicit phase-one unsupported ED2K errors, HTTP POST `/jsonrpc`, and WebSocket notification delivery on `/jsonrpc`. The RPC transport uses `axum + serde_json` so raria keeps full control over aria2-compatible token and response shaping. `jsonrpsee` is recorded as declined for phase one because it owns too much of the method/error surface for this compatibility layer.

Current checkpoint: `40-http-engine`.

Next action: implement HTTP(S) new-task download execution behind the existing RPC task model, starting with a local HTTP fixture for add/poll/pause/resume/save.

Started checkpoint `40-http-engine`. The HTTP adapter now uses `reqwest` for single-source HTTP(S) downloads behind the existing RPC task model. A local axum fixture proves `aria2.addUri` can create a task, `DownloadEngine::run_once` downloads it to the configured directory, and `aria2.tellStatus` reports completion. A second fixture proves basic `.raria` resume behavior: raria reads a JSON control sidecar with `completedLength`, sends an HTTP Range request, appends the remaining bytes, removes the control file on completion, and updates completed length.

Remaining in `40-http-engine`: true range splitting, proxy/header/cookie/netrc option coverage, checksum verification, and rate limiting.

Added whole-file SHA-256 checksum verification for HTTP downloads. A mismatch now marks the task as `error`, emits the existing error path, and exposes an `errorMessage` through `aria2.tellStatus`.

Remaining in `40-http-engine`: true range splitting, proxy/header/cookie/netrc option coverage, and rate limiting.

Added HTTP request option coverage for per-task `header` and a simple `load-cookies` file, both applied through reqwest. Added task-level `max-download-limit` enforcement through governor with a focused local fixture. `40.03` remains in progress until proxy and netrc behavior are covered.

Added focused range splitting support for HTTP downloads. For `split > 1`, raria probes `Content-Range` with a small range request, then performs sequential range requests and joins the bytes into the output file. This keeps the first implementation deterministic while preserving the external aria-style split behavior contract.

Added `netrc-path` support through the mature `netrc` crate and reqwest basic auth. Added `http-proxy` support through reqwest `Proxy::http` with a local proxy fixture that proves requests route through the proxy. Completed checkpoint `40-http-engine`.

Current checkpoint: `50-ftp-sftp`.

Next action: probe mature FTP and SFTP crates with controlled fixtures, then implement the minimal new-task adapter path that feeds the existing RPC status model.

Completed checkpoint `50-ftp-sftp`. The download engine now routes `ftp://` tasks through `suppaftp` and `sftp://` tasks through `russh-sftp + russh`. Controlled local fixtures cover basic download, `.raria` resume by completed length, status completion, and `ftp-user` plus `ftp-passwd` credential options. `libunftp + unftp-sbe-fs` is accepted as the FTP fixture stack. FTPS, FTP proxy behavior, and SFTP host-key pinning remain recorded probes rather than fake compatibility.

Current checkpoint: `60-bittorrent`.

Next action: probe the BitTorrent library path, with extra scrutiny because the current `librqbit` candidate is published as a release candidate.

Started checkpoint `60-bittorrent`. The stable metadata layer now uses `bendy` for torrent bencode decoding, RustCrypto `sha1` for raw info-hash calculation, and `url` for magnet parsing. RPC now accepts `aria2.addTorrent` base64 torrent metadata and magnet `aria2.addUri` tasks, exposes `infoHash`, `bittorrent.info.name`, torrent file lists through `tellStatus` and `aria2.getFiles`, and maps `select-file` into per-file selected status. Full peer transfer remains unresolved because `librqbit` and its core crates are still published as `9.0.0-rc.0`; continue probing before accepting it as the engine.

Completed checkpoint `60-bittorrent`. The transfer adapter now uses stable `librqbit` 8.1.1 rather than the 9.0.0 release candidate. Local multi-threaded fixtures prove both `aria2.addTorrent` torrent bytes and magnet `aria2.addUri` can download from an initial local peer into the configured raria directory and mark the RPC task complete. raria keeps only the compatibility/task orchestration layer: base64 torrent ingestion, magnet metadata status, selected-file mapping, `bt-initial-peer` fixture support, and completion status shaping.

Current checkpoint: `70-metalink`.

Next action: implement Metalink v3/v4 parsing with `quick-xml`, map Metalink entries into new download tasks, and keep unsupported legacy Metalink behavior explicit.

Completed checkpoint `70-metalink`. The Metalink parser now uses `quick-xml` for v3 and v4 file entries, size, SHA-256 whole-file hashes, and HTTP resource URLs. `aria2.addMetalink` accepts base64 Metalink bytes, creates ordinary HTTP download tasks, applies the mapped checksum, returns aria2-style gid arrays, and reuses the existing HTTP engine path.

Current checkpoint: `80-persistence`.

Next action: tighten `.raria` control-file semantics for new-task persistence and restart-style resume without old `.aria2` migration.

Completed checkpoint `80-persistence`. The core now has a versioned `.raria` control model with atomic JSON writes, public read/write helpers, and restart-style HTTP resume through the same control file used by protocol fixtures. Completion cleanup removes the `.raria` sidecar after successful HTTP, FTP, and SFTP downloads. The schema explicitly documents phase-one completed-byte storage and keeps old `.aria2` migration out of scope.

Current checkpoint: `90-release-readiness`.

Next action: audit phase-one contract gaps, tighten user-facing scope docs, run final verification, and leave a release-readiness checkpoint with remaining post-phase-one probes clearly recorded.

Started checkpoint `90-release-readiness`. The release audit found stale RPC contract statuses and a practical new-session polling gap around token stripping plus common query methods. The core now supports token-prefixed RPC parameters, `aria2.getUris`, `aria2.tellActive`, `aria2.tellWaiting`, `aria2.tellStopped`, `aria2.getVersion`, `aria2.getSessionInfo`, and `aria2.saveSession` acknowledgement. Phase-one scope is summarized in `release-scope.md`.

Completed checkpoint `90-release-readiness`. Final verification passed for formatting, clippy, workspace tests, and goal document parsing. Phase-one scope is documented, pruning decisions are current, and the remaining non-phase-one probes are explicit rather than silently fake-compatible.
