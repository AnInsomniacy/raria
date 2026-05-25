# Real End-to-End Smoke Report

Date: 2026-05-25
Binary: `var/releases/macos/raria`
Version: `raria 0.1.0`
Platform: macOS arm64
SHA-256: `e2208808cdc86668ea8cb863c75b0d822213029f459143d403acf790cb09ed92`
Evidence root: `var/smoke-runs/20260525T104946Z-e2e`

## Scope

This run cleared `var/`, rebuilt the release binary, created a fresh smoke directory, and exercised the release binary through CLI, daemon mode, native `/api/v1` HTTP JSON resources, `/api/v1/events`, local controlled fixtures, and the three maintainer-selected public inputs.

Old JSON-RPC was not tested because it has been deleted. RPC coverage means the native HTTP JSON API and WebSocket event stream.

## Commands

```bash
find var -mindepth 1 ! -name README.md -exec rm -rf {} +
mkdir -p var/releases/macos var/smoke-runs var/tmp
cargo build --release -p raria-cli
cp target/release/raria var/releases/macos/raria
shasum -a 256 var/releases/macos/raria > var/releases/macos/raria.sha256
node var/tmp/e2e-smoke.cjs
cargo test -p raria-cli --test single_download
cargo test -p raria-cli --test session_smoke
cargo test -p raria-cli --test native_api_smoke
cargo test -p raria-cli --test sftp_smoke
cargo test -p raria-ftp --test ftp_smoke
cargo test -p raria-ftp --test ftps_smoke
cargo test -p raria-bt --test bt_smoke
cargo test -p raria-bt --test bt_gap_ledger
cargo test -p raria-bt --test dht_persistence
historical daemon BT smoke command removed from the default suite
```

## API Surface

`/api/v1/health`, `/api/v1/config`, `/api/v1/stats`, `/api/v1/transfer`, `/api/v1/tasks`, `/api/v1/tasks/{taskId}`, `/api/v1/tasks/{taskId}/pause`, `/api/v1/tasks/{taskId}/resume`, `/api/v1/tasks/{taskId}/restart`, `/api/v1/tasks/{taskId}/queue`, `/api/v1/tasks/{taskId}/sources`, `/api/v1/tasks/{taskId}/transfer`, `/api/v1/session/save`, and `/api/v1/daemon/shutdown` returned successful native responses during release-binary smoke.

`/api/v1/events` streamed native task lifecycle and progress events, including `task.created`, `task.started`, `task.progress`, `task.paused`, `task.resumed`, `task.removed`, and `task.completed`. Session save and restore worked with 4 restored tasks. No daemon process remained after shutdown checks.

## Public Input Results

Apple HTTPS passed. The IPSW task reached `25,468,906` bytes in the first bounded window and `34,444,074` bytes after resume. It reported `238,319,275` total bytes, peaked at `1,890,388,831` B/s, showed 8 active connections, and accepted pause, resume, session save, and shutdown. Evidence: `reports/public-https-apple-summary.json`.

Magnet metadata passed. The KNOPPIX CD magnet resolved metadata in about 5 seconds, exposed 9 files and `700,612,589` total bytes, transitioned to `paused` under metadata-only policy, and repeated pause returned `200` with `paused`. This confirms `SMOKE-006` is fixed in release-binary E2E. Evidence: `reports/public-magnet-metadata-summary.json`.

Torrent-file bounded download passed. The KNOPPIX DVD torrent exposed 9 files, 2 trackers, and 15 peers. File selection kept only `file_0` selected. The task downloaded `2,097,152` bytes in the bounded window, peaked at `123,592` B/s, showed 15 active connections, and accepted pause, resume, session save, and shutdown. Evidence: `reports/public-torrent-file-summary.json`.

## Controlled Fixture Results

CLI basics passed. Help, download help, bash completion, strict `raria.toml` loading, custom header, checksum verification, and local range download all succeeded. The downloaded SHA-256 matched `bea2b4efdafb6f195db10d6480b2c1a79b7044f7125c3a7ed371a93421e454c7`. Evidence: `reports/cli-basics-summary.json`.

Native API control passed. Local HTTP range completed `4,194,304` bytes with matching SHA-256, 12 fixture range requests, custom header propagation, `13,348,895` B/s sampled speed, slow-task pause/resume/restart/remove, idempotent repeated pause, queue patch, source replacement, per-task transfer patch, global transfer patch, event stream, session save, restore, and shutdown. Evidence: `reports/api-control-summary.json`.

Protocol smoke passed. `single_download` passed 24 tests, `session_smoke` passed 19 tests, `native_api_smoke` passed 30 tests, CLI `sftp_smoke` passed 3 tests, FTP smoke passed 3 tests, FTPS smoke passed 1 test, BT smoke passed 11 tests, BT gap ledger passed 3 tests, DHT persistence passed 3 tests, and Duplicate daemon BT tracker smoke was later removed from the default suite.

## Bug Ledger

No open bug was found in this run.

`SMOKE-006` remains fixed. The release-binary magnet metadata smoke repeated `/api/v1/tasks/{taskId}/pause` on an already paused metadata-only task and received `200` with `paused`.

No regression was observed for bounded HTTP ranges, nested output directories, native transfer speed, WebSocket event delivery, Apple HTTPS progress, torrent tracker/peer projection, file selection, session save/restore, protocol smoke, or daemon shutdown.

## Artifact Hygiene

Generated binaries, smoke scripts, session databases, logs, raw API payloads, and partial downloads are under `var/`, which is ignored except `var/README.md`. No matching temporary runtime artifacts were found outside `var/`. Process inspection found no release daemon remaining after the run.

## Next Engineering Target

Continue with the core modernization tracker. No E2E-blocking bug is currently documented from the latest smoke run.
