# Contributing to raria

Thank you for your interest in contributing to raria!

## Development Setup

```bash
# Clone
git clone https://github.com/AnInsomniacy/raria.git
cd raria

# Build
cargo build --workspace

# Test (must pass before any PR)
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Quality Standards

All contributions must meet these requirements:

1. **Tests pass** — `cargo test --workspace` with 0 failures
2. **Clippy clean** — `cargo clippy --workspace -- -D warnings` with 0 warnings
3. **TDD workflow** — write tests first, then implementation
4. **No stub tests** — tests must exercise real logic, not just constructor validation

## Code Organization

| Crate | Purpose |
|-------|---------|
| `raria-core` | Job model, engine, scheduler, persistence, config, checksum |
| `raria-range` | `ByteSourceBackend` trait + `SegmentExecutor` |
| `raria-http` | HTTP/HTTPS backend (reqwest) |
| `raria-ftp` | FTP/FTPS backend (suppaftp) |
| `raria-sftp` | SFTP backend (russh) |
| `raria-bt` | BitTorrent service (librqbit) |
| `raria-metalink` | Metalink XML parser |
| `raria-rpc` | JSON-RPC server (jsonrpsee) |
| `raria-cli` | CLI binary (clap) |

## Pull Request Process

1. Fork the repository
2. Create a feature branch from `main`
3. Write tests first (TDD)
4. Implement the feature
5. Ensure all quality gates pass
6. Submit a PR with a clear description

## Architecture Notes

- **Protocols**: HTTP/FTP/SFTP implement `ByteSourceBackend` (range-based). BT uses `BtService` (session-based).
- **Concurrency**: `SegmentExecutor` uses `tokio::spawn` + `Semaphore` for controlled parallelism.
- **Persistence**: All job state changes persist to redb before in-memory update.
- **Cancellation**: `CancelRegistry` manages per-job `CancellationToken` trees. Pause/shutdown propagates via child tokens.

## License

By contributing, you agree that your contributions will be licensed under Apache 2.0.
