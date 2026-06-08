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
