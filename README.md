# raria

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-workspace%20verified-brightgreen.svg)](#testing)

A Rust download engine with a practical, capability-first maturity model.

> The active execution contract is **light governance + progressive capability maturity**. Public-facing guidance lives in [`docs/practical-maturity.md`](docs/practical-maturity.md). The detailed planning source of truth remains `.omx/plans/2026-04-11-raria-practical-maturity-plan.md`.

## Current Capability Snapshot

| Capability | Status | Notes |
| --- | --- | --- |
| HTTP/HTTPS segmented downloads | ✅ Verified | `reqwest` backend with range support, retries, cookies, proxy support, and checksum hooks |
| FTP/FTPS downloads | ✅ Verified | `suppaftp` backend with smoke coverage in CLI/backend tests |
| SFTP downloads | ✅ Verified | `russh`/`russh-sftp` backend with smoke coverage |
| Metalink parsing | ✅ Verified | v3/v4 parser and normalizer are in-tree |
| aria2-style JSON-RPC daemon | ✅ Verified | HTTP + WebSocket server, queue/status/global-stat flow, daemon lifecycle tests |
| Session persistence / restore | ✅ Verified | redb-backed store and daemon/session smoke coverage |
| BitTorrent service integration | ⚠️ In progress | `raria-bt` and daemon BT flow exist, but seeding semantics, honest facade projection, and some stop-line gaps are still being closed |
| XML-RPC | ❌ Not supported | Permanently out of scope |

## Practical Maturity Model

raria is intentionally not following a "rebuild the foundation first" plan. The current execution model is:

1. **Keep the target unchanged** — raria becomes a mature Rust download engine, with download-core and BitTorrent as the top priorities.
2. **Prefer real capability output** — each step should land user-visible or testable behavior, not just internal reshuffling.
3. **Keep governance light but real** — only three hard gates remain:
   - stop-line grading
   - dependency viability audit
   - write-scope / crate-boundary discipline
4. **Do local refactors only when blocked** — internal cleanup is allowed only when the current structure clearly prevents the next capability from landing honestly.
5. **Treat aria2 compatibility as a migration facade** — it must project internal truth, not redefine it.

See [`docs/practical-maturity.md`](docs/practical-maturity.md) for the English companion document that translates the active execution plan into repo-facing guidance.

## Quick Start

### Single-file download

```bash
# Download with 16 concurrent connections
raria download https://example.com/large-file.zip -x 16

# Custom output directory and filename
raria download https://example.com/file.zip -d ~/Downloads -o myfile.zip

# With speed limit (1 MB/s)
raria download https://example.com/file.zip --max-download-limit 1048576

# With checksum verification
raria download https://example.com/file.zip --checksum sha-256=e3b0c44298fc...
```

### Daemon mode (aria2-compatible RPC)

```bash
# Start daemon with RPC on port 6800
raria daemon -d ~/Downloads --rpc-port 6800

# Add a download via JSON-RPC
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "aria2.addUri",
    "id": "1",
    "params": [["https://example.com/file.zip"], {"dir": "/tmp"}]
  }'

# Query status
curl -X POST http://127.0.0.1:6800/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"aria2.tellStatus","id":"2","params":["0000000000000001"]}'
```

## Building from Source

```bash
git clone https://github.com/AnInsomniacy/raria.git
cd raria
cargo build --release
```

The binary is emitted at `target/release/raria`.

## Workspace Layout

raria is a Cargo workspace with focused crates rather than one monolith:

```text
raria/
├── crates/
│   ├── raria-core      # Job model, engine, scheduler, persistence, config, checksum
│   ├── raria-range     # ByteSourceBackend trait + concurrent SegmentExecutor
│   ├── raria-http      # HTTP/HTTPS backend (reqwest)
│   ├── raria-ftp       # FTP/FTPS backend (suppaftp)
│   ├── raria-sftp      # SFTP backend (russh / russh-sftp)
│   ├── raria-bt        # BitTorrent service (librqbit)
│   ├── raria-metalink  # Metalink parser / normalizer
│   ├── raria-rpc       # aria2-style JSON-RPC server / facade
│   └── raria-cli       # CLI + daemon integration layer
├── docs/
│   └── practical-maturity.md
├── Cargo.toml
└── README.md
```

## Architecture Notes

- **Library-first:** protocol/runtime logic lives in library crates; `raria-cli` wires the CLI and daemon.
- **Protocol split:** HTTP/FTP/SFTP share the range-download path; BitTorrent is session/piece driven and intentionally separate.
- **Persistence-first engine state:** job state is stored in redb and restored through the core engine.
- **Facade honesty:** aria2-style responses should project internal truth instead of forcing the core model to imitate aria2 internals.
- **Capability-first delivery:** roadmap steps are evaluated by shipped behavior and tests, not abstract architecture completeness.

## Current Maturity Sequence

The active plan advances in this order:

1. Minimal core semantics for BT seeding and shared events
2. BT field synchronization and one-shot download-complete semantics
3. Daemon checksum / conditional-get capability closure
4. Honest RPC / facade projection of real BT fields
5. Metalink execution-path upgrades
6. Full verification closure plus docs that describe only real capabilities

Each step has an explicit primary write scope, allowed support crates, and forbidden write zones. See [`docs/practical-maturity.md`](docs/practical-maturity.md) for the condensed contract.

## Testing

```bash
# Full workspace tests
cargo test --workspace

# Full type-check without producing release artifacts
cargo check --workspace

# Lint with warnings denied
cargo clippy --workspace --all-targets -- -D warnings
```

## Related Projects

- [Motrix Next](https://github.com/AnInsomniacy/motrix-next) — a Tauri/Vue download manager that can consume aria2-style control surfaces
- [aria2-builder](https://github.com/AnInsomniacy/aria2-builder) — cross-platform aria2 builds
- [aria2](https://github.com/aria2/aria2) — the original C++ download utility

## License

Apache 2.0 — see [LICENSE](LICENSE).
