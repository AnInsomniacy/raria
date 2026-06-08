# Raria Goal

Use this goal text when starting the long-running Codex goal.

```text
/goal In /Users/sekiro/Projects/personal/raria on the current branch, build raria from an empty repository into a new pure-Rust download engine that offers low-friction replacement for aria2-next new-session workflows. Treat this as a new project, not a legacy state migration. Do not create a new branch. Do not use subagents. Do not reuse legacy/raria or any old branch. First read and maintain docs/raria-goal/ as the durable project memory, updating progress.md, checkpoint-index.csv, the active checkpoints/*.csv file, and the relevant ledgers after each checkpoint.

The subjective product target is a clean, modern, principled Rust implementation that feels seamless for new tasks: an existing aria2-next-oriented app or script should be able to start raria, add new downloads, poll JSON-RPC status, pause and resume downloads, remove tasks, read global stats, receive WebSocket events, and use common CLI/config/input-file flows with minimal changes. Existing .aria2 control files, old aria2 sessions, old queues, old partial task state, old caches, and old download history are out of scope.

Prefer mature, stable Rust libraries wherever they fit. Do not hand-roll HTTP, TLS, XML parsing, bencode, FTP, SFTP, rate limiting, serialization, hashing, or BitTorrent core behavior if a suitable maintained library works. Hand-write only raria-specific task orchestration, compatibility semantics, .raria state, RPC response shaping, library adapters, and protocol gaps with no mature library. Use objective probes before rejecting a mature library.

Use a modern internal architecture. Prefer Tokio actors or event-driven task state machines over cloning aria2's C++ Command/EventPoll polling model. External behavior matters more than internal similarity.

Phase one must implement or track: common CLI/config parsing, input-file parsing, save-session for new tasks, JSON-RPC over HTTP, JSON-RPC over WebSocket notifications, HTTP(S) multi-source range downloads and resume, FTP, SFTP, BitTorrent magnet/torrent support, Metalink support, new .raria control files, and focused verification fixtures. ED2K is not implemented in phase one; ED2K CLI options, ed2k:// URIs, aria2.ed2kSearch, aria2.getEd2kSearchResults, and ED2K status fields must be tracked and return explicit unsupported behavior.

Prune old surfaces that do not serve the new-session replacement target: XML-RPC, rpc-user/rpc-passwd authentication, HTTP pipelining, event-poll backend selection, old .aria2 state migration, Autotools/CMake legacy, hand-written platform probe layers, and broad noise tests. Keep pruning decisions in decision-ledger.csv with user impact and replacement behavior.

Testing must be minimal but meaningful. Add tests for CLI/config parsing, RPC request and response shapes, WebSocket event shapes, .raria persistence, input/save-session behavior, protocol parser boundaries, and high-risk task state transitions. Avoid tests that only mirror implementation details or require public network conditions as gates.

Stop when raria builds, cargo fmt passes, cargo clippy passes, cargo test passes, the phase-one contract fixtures pass, HTTP(S), FTP, SFTP, BitTorrent, and Metalink have working new-task paths, .raria save/resume works for new tasks, JSON-RPC polling and WebSocket events are smooth for new sessions, ED2K returns explicit unsupported behavior, and docs/raria-goal/ plus user-facing notes explain the phase-one scope.
```

