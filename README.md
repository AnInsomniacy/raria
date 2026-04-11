# raria

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)

A Rust download engine with a practical, capability-first maturity model.

> The active repository-facing contract is no longer the old six-step future roadmap. The source-of-truth plan is `.omx/plans/2026-04-11-raria-progress-adjusted-maturity-plan.md`, with the public companion docs in [`docs/practical-maturity.md`](docs/practical-maturity.md), [`docs/bt-stop-lines.md`](docs/bt-stop-lines.md), and [`docs/verification-contract.md`](docs/verification-contract.md).

## Current Capability Snapshot

| Capability area | Current state | Evidence anchor |
| --- | --- | --- |
| HTTP/HTTPS segmented downloads | Baseline with automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| FTP/FTPS downloads | Baseline with automated coverage | backend and CLI workspace tests |
| SFTP downloads | Baseline with automated coverage | backend and CLI workspace tests |
| aria2-style JSON-RPC daemon | Baseline with automated coverage | `crates/raria-cli/tests/rpc_smoke.rs`, `crates/raria-rpc/tests/` |
| Session persistence and restart restore | Baseline with automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| Conditional GET and checksum enforcement | Baseline with automated coverage | `crates/raria-cli/tests/session_smoke.rs` |
| Metalink parsing, normalization, checksum wiring, relation projection, and mirror failover | Baseline with automated coverage | `crates/raria-rpc/tests/metalink_dispatch.rs`, `crates/raria-cli/tests/session_smoke.rs` |
| BitTorrent lifecycle semantics and facade projection | Baseline with explicit limits | `crates/raria-core/src/job.rs`, `crates/raria-cli/src/bt_runtime.rs`, `crates/raria-rpc/src/facade.rs` |
| BitTorrent parity beyond the current baseline | Explicit stop-line gaps remain | `crates/raria-bt/tests/bt_gap_ledger.rs` |
| XML-RPC | Out of scope | permanently unsupported |

## Current Stage

The old Step 1 through Step 4 work should now be treated as baseline, not as pending roadmap:

- core BT semantics already include a distinct `Seeding` state and a one-shot BT completion guard
- the daemon and RPC surface already cover session restore, conditional GET, checksum failure reporting, and aria2-style status/global-stat flows
- Metalink already parses, normalizes, stores checksum and relation data, and projects that data through RPC
- the BitTorrent path is already real in the daemon, runtime, and facade layers, but it still carries explicit parity stop-lines that must stay documented honestly

The active closeout work is narrower:

1. keep repo-facing docs aligned with the implemented baseline
2. keep BitTorrent stop-lines, RPC behavior, and docs synchronized
3. keep Metalink runtime claims aligned with the daemon-path evidence now present in the test suite
4. require fresh verification evidence before any late-stage closure claim

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
│   ├── practical-maturity.md
│   └── verification-contract.md
├── Cargo.toml
└── README.md
```

## Architecture Notes

- Library-first: protocol and runtime logic live in library crates; `raria-cli` wires the CLI and daemon.
- Protocol split: HTTP, FTP, and SFTP share the range-download path; BitTorrent stays session and piece driven.
- Persistence-first engine state: job state is stored in redb and restored through the core engine.
- Facade honesty: aria2-style responses project internal truth instead of forcing the core model to imitate aria2 internals.
- Capability-first delivery: remaining work is judged by shipped behavior, explicit stop-lines, and versioned verification assets.

## Verification

The repository keeps a versioned verification contract instead of a static "everything is green" claim. See [`docs/verification-contract.md`](docs/verification-contract.md) for:

- the required verification matrix
- the critical path to test-file mapping
- which claims are durable repository facts versus fresh-run claims
- the remaining evidence gaps for late-stage closure

BitTorrent parity limits are tracked separately in [`docs/bt-stop-lines.md`](docs/bt-stop-lines.md).

## Related Projects

- [Motrix Next](https://github.com/AnInsomniacy/motrix-next) - a Tauri/Vue download manager that can consume aria2-style control surfaces
- [aria2-builder](https://github.com/AnInsomniacy/aria2-builder) - cross-platform aria2 builds
- [aria2](https://github.com/aria2/aria2) - the original C++ download utility

## License

Apache 2.0. See [LICENSE](LICENSE).
