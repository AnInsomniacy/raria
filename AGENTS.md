# AGENTS.md - raria

This file defines repository rules for AI coding agents. Human contributors should start with `README.md`, `CONTRIBUTING.md`, and `docs/core-modernization/overview.md`.

All changes must meet production-grade Rust quality. Find the root cause before changing behavior, keep the public contract raria-native, avoid unrelated churn, and verify the exact path affected by the change.

## Product Contract

raria is a Rust-native download manager and daemon. It is not an aria2 compatibility layer.

Supported public surfaces are `raria.toml`, the `/api/v1` HTTP JSON API, the `/api/v1/events` WebSocket stream, opaque raria task identifiers, versioned native persistence schemas, native CLI names, structured logs, generated shell completion, release binaries, and native documentation.

Do not add JSON-RPC, XML-RPC, aria2 method names, aria2 option names, Gid-facing public behavior, aria2 config syntax, aria2 session or control-file compatibility, AriaNg or Motrix legacy adapters, HTTP pipelining, BitTorrent MSE or ARC4, LPD, ED2K, browser-cookie import, or historical platform baggage.

Future GUI clients may integrate with raria, but they must adapt to raria's native API and event model.

## Architecture

| Area | Ownership |
| --- | --- |
| Core | `crates/raria-core` owns task state, scheduling, cancellation, persistence, config, checksum, and lifecycle events |
| Range transfers | `crates/raria-range` owns segmented transfer orchestration over protocol backends |
| HTTP and HTTPS | `crates/raria-http` uses reqwest for modern HTTP behavior |
| FTP and FTPS | `crates/raria-ftp` uses suppaftp for FTP-family transfers |
| SFTP | `crates/raria-sftp` uses russh and russh-sftp |
| Metalink | `crates/raria-metalink` owns retained Metalink 4 parsing and normalization |
| BitTorrent | `crates/raria-bt` uses librqbit for torrent, magnet, DHT, tracker, peer, piece, and fastresume behavior where public APIs support it |
| Native API | `crates/raria-rpc` owns `/api/v1` resources and WebSocket events |
| CLI and daemon | `crates/raria-cli` owns command parsing, daemon wiring, hooks, and release binary entry points |
| Modernization evidence | `docs/core-modernization` owns durable capability, dependency, and validation records |
| Local generated output | `var/` is the only repository-root scratch area and is ignored except `var/README.md` |

## Rust Rules

Use the pinned stable toolchain from `rust-toolchain.toml`. Keep the workspace on Rust 2024 and the workspace `rust-version` unless a deliberate project-wide baseline change is made.

Prefer mature libraries over local protocol implementations. Current preferred owners include Tokio, reqwest, suppaftp, russh, russh-sftp, librqbit, redb, axum, serde_json, quick-xml, clap, governor, rustls, and tracing. If a library cannot cover required modern behavior, implement the smallest raria-native layer and document the limitation or ownership decision in `docs/core-modernization/dependency-ledger.csv`.

Do not read dependency source code or generated build output during normal work. Use official documentation, docs.rs, RFCs, or upstream repositories only when current API behavior or protocol facts cannot be proven locally.

## Testing

Tests must be restrained and high value. Add tests for native public contracts, protocol boundaries, persistence and recovery, transfer truth, security-sensitive behavior, release-critical behavior, and confirmed regressions.

Do not add parity tests, compatibility-only tests, broad scaffolding, fragile public-network gates, or tests for removed behavior. Prefer focused unit, contract, integration, or smoke coverage that proves one durable behavior.

Use the smallest relevant verification command during development. Expand to the full ladder for shared behavior, release preparation, and completion claims:

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

When Python is needed, use:

```bash
source /opt/anaconda3/etc/profile.d/conda.sh && conda activate global
```

## Documentation

Keep documentation synchronized with behavior. If an API route, event field, CLI flag, config key, dependency limit, release artifact, validation claim, or supported platform changes, update the relevant documentation in the same change.

`docs/core-modernization` is for durable audit and validation records. Do not commit raw API payloads, temporary logs, downloaded files, generated release folders, local caches, or network scratch data.

`CHANGELOG.md` is historical project prose, not the release gate. GitHub Release notes are the authoritative user-facing release record. Update `CHANGELOG.md` only when a maintainer explicitly asks for it or when the repository intentionally re-adopts changelog-driven releases.

## Version Management

`Cargo.toml` under `[workspace.package]` is the single source of truth for the project version. Workspace crates inherit it through `version.workspace = true`.

Use `./scripts/bump-version.sh <major.minor.patch>` to change the workspace version. The script accepts only plain numeric Semantic Versioning releases and keeps Cargo metadata locked.

Use normal SemVer arithmetic. A minor bump from `1.0.6` is `1.1.0`; a patch bump from `1.0.6` is `1.0.7`.

Release tags use `v{workspace.version}`. The tag version and Cargo workspace version must match exactly after removing the leading `v`. Pre-release, beta, RC, channel, build-metadata, and date-based release suffixes are not supported unless the maintainer changes the release policy first.

Treat published release tags as immutable. If a failed release has not been consumed, delete the failed GitHub Release and tag, fix the commit, then recreate the same version deliberately. If a release has been publicly consumed, stop and ask the maintainer for the exact next version.

## Release Process

The release workflow is `.github/workflows/release.yml`. It builds release archives for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, Windows x86_64, and Windows aarch64, then uploads checksummed assets to a GitHub Release.

Local release preparation must be driven by two scripts:

```bash
./scripts/bump-version.sh <major.minor.patch>
./scripts/release.sh
```

`bump-version.sh` owns version edits only. `release.sh` owns local release verification, release binary build, version and tag checks, optional release commit creation, and annotated tag creation. It must not create GitHub Release notes. It must not publish assets. It must not push unless the maintainer explicitly requests that behavior.

Before a release tag is created, verify locally:

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --locked -p raria-cli
target/release/raria --version
```

For a major release or release-process change, also refresh real smoke evidence when requested by the maintainer. Keep raw smoke output under `var/`; commit only concise durable conclusions.

After the tag is ready, write a concise English GitHub Release title and notes from the commits and verified behavior in the release. The notes are the official release record. They should be written for users and integrators, mention breaking changes before general changes, and include the produced artifacts and checksums once available.

Release title format:

```text
v{VERSION} - {Concise Release Theme}
```

Use only relevant sections in release notes:

```markdown
## Summary

### Added

### Changed

### Fixed

### Security

### Breaking Changes

### Downloads
```

Omit empty sections. Avoid raw commit dumps, future promises, internal-only chores, and unsupported claims.

## Failed Release Recovery

If the GitHub Release has not been created, remove an incorrect local and remote tag only after the maintainer confirms the exact tag.

If the GitHub Release exists and the release workflow failed before public consumption, delete the failed GitHub Release, delete the local and remote tag, fix the commit, rerun verification, and recreate the same tag deliberately.

If the release has been publicly consumed, do not delete the release, delete the tag, or invent a replacement version. Stop and report the exact failure state.

## Git Rules

Work on the current branch unless the maintainer explicitly asks for a new branch. Do not push remote changes unless explicitly asked.

Never revert user changes or unrelated work. Do not use destructive git commands unless the maintainer explicitly requests that exact operation.

Commit only meaningful verified boundaries such as a completed release-preparation step, a native surface replacement, a protocol/runtime fix, a stale-surface deletion, or a documentation rule that changes future work. Commit messages must be concise professional English.

## Temporary Files

Use `var/` for local smoke runs, release staging, generated binaries, scratch payloads, session stores, logs, and downloaded fixtures. Keep generated output out of repository roots and crate directories.

Before claiming a clean state, ensure temporary output is either intentionally retained under ignored `var/` or removed.
