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
