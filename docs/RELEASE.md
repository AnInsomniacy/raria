# Release Process

`Cargo.toml` under `[workspace.package]` is the single source of truth for the
raria version. Release tags use `v{version}` and must match the workspace
version exactly after removing the leading `v`.

## Local Preparation

Use the release scripts instead of editing version strings or creating tags by
hand.

```bash
./scripts/bump-version.sh 1.0.0
./scripts/release.sh
```

`bump-version.sh` changes only the workspace version and refreshes Cargo
metadata. `release.sh` verifies the release boundary, builds the release binary,
checks the binary version, creates an optional version commit when version files
changed, and creates an annotated local tag.

`release.sh` does not create GitHub Release notes, publish assets, or push
unless `--push` is passed deliberately.

## Required Local Gate

The local release gate is:

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --locked -p raria-cli
target/release/raria --version
```

Public-network smoke tests are not a release gate unless a maintainer explicitly
requests them. Keep raw smoke output under ignored `var/` and commit only
concise durable conclusions.

## GitHub Release Workflow

`.github/workflows/release.yml` builds release archives for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `aarch64-pc-windows-msvc`

Each archive is uploaded with a `.sha256` file. The workflow is triggered by a
tag push matching `v*` or manual workflow dispatch. Manual dispatch creates a
draft release tag name for artifact inspection.

## Release Notes

GitHub Release notes are the authoritative user-facing release record. Write
them from verified behavior and merged commits. Mention breaking changes before
general changes. Do not paste raw commit dumps, future promises, local smoke
logs, or unsupported claims.

Recommended title format:

```text
v1.0.0 - Native Download Manager Foundation
```

Use only sections that have content:

```markdown
## Summary

### Added

### Changed

### Fixed

### Security

### Breaking Changes

### Downloads
```

## Failed Release Recovery

Treat published tags as immutable. If a release has not been consumed, delete
the failed GitHub Release and tag only after the maintainer confirms the exact
recovery path. If a release has been publicly consumed, keep the release intact
and ask the maintainer for the next version.
