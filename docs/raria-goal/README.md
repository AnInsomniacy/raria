# Raria Goal Tracking

This directory is the durable tracking record for building `raria` as a new Rust download engine with low-friction migration from `aria2-next` for new work.

`raria` is not a historical-state migration project. Existing `.aria2` control files, old aria2 sessions, old download queues, old caches, and old partial task state are out of scope. The migration target is practical replacement for new tasks: users and RPC clients should be able to start `raria`, add new downloads, poll status, pause or resume tasks, receive events, and continue normal new-session workflows with as few changes as possible.

The first phase keeps the new-task surfaces that matter most: common CLI options, configuration parsing, input files, save-session for new tasks, JSON-RPC over HTTP, JSON-RPC over WebSocket notifications, HTTP(S), FTP, SFTP, BitTorrent, Metalink, and the new `.raria` control file. ED2K is tracked as a later phase and must return explicit unsupported behavior in phase one.

Implementation must prefer mature, stable Rust libraries. Hand-written code is allowed only for `raria`-specific orchestration, compatibility semantics, `.raria` state, RPC response shaping, focused adapters, and protocol pieces without a suitable maintained library. The internal architecture may be completely different from aria2. A Tokio-based actor or event-driven design is preferred over cloning aria2's old command polling model.

Read order after context compaction: `progress.md`, then `checkpoint-index.csv`, then the active file under `checkpoints/`. Use `contract-ledger.csv` and `contracts/` for external behavior. Use `library-ledger.csv` before implementing any protocol or subsystem. Use `decision-ledger.csv` before revisiting a major architecture or pruning decision.

