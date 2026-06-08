# Progress

## 2026-06-08

Created the tracking structure for the long-running raria goal. The current project position is a new Rust project, not a legacy aria2 state migration. Phase one targets low-friction new-session replacement: JSON-RPC polling, WebSocket events, common CLI/config/input-file flows, HTTP(S), FTP, SFTP, BitTorrent, Metalink, save-session for new tasks, and `.raria` persistence. ED2K is deferred and must return explicit unsupported behavior.

Current checkpoint: `00-discovery`.

Next action: start the goal, then fill the discovery checkpoint from `aria2-next` sources and lock the first contract ledger pass before writing Rust source.

