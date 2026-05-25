# Real End-to-End Smoke Report

Date: 2026-05-25
Binary: `var/releases/macos/raria`
Version: `raria 0.1.0`
Platform: macOS arm64
SHA-256: `8cc5dae3c5ad9077d86fe0341ab4ba78514f3e033845cc5646b94c6baef9a2fd`
Evidence root: `var/smoke-runs/20260525-100155-clean-e2e`

## Scope

This run cleared `var/`, rebuilt the release binary, created a fresh smoke directory, started native daemons on ephemeral ports, exercised CLI, native HTTP JSON API, WebSocket events, local controlled HTTP fixtures, and the three maintainer-selected public inputs. Large public downloads were bounded.

Old JSON-RPC was not tested because it has been deleted. RPC coverage means `/api/v1` HTTP JSON resources and `/api/v1/events`.

## Commands

```bash
find var -mindepth 1 ! -name README.md -exec rm -rf {} +
cargo build --release -p raria-cli
cp target/release/raria var/releases/macos/raria
shasum -a 256 var/releases/macos/raria > var/releases/macos/raria.sha256
node var/tmp/e2e-smoke.cjs
node var/tmp/api-control-smoke.cjs
cargo test -p raria-cli --test single_download
cargo test -p raria-cli --test session_smoke
cargo test -p raria-cli --test sftp_smoke
cargo test -p raria-ftp --test ftp_smoke
cargo test -p raria-ftp --test ftps_smoke
```

## API Surface

`/api/v1/health`, `/api/v1/config`, `/api/v1/stats`, `/api/v1/tasks`, `/api/v1/transfer`, `/api/v1/session/save`, and `/api/v1/daemon/shutdown` returned successful native JSON responses. `/api/v1/events` streamed `task.created`, `task.started`, `task.progress`, and `task.completed` for a controlled HTTP task.

Task control coverage passed for create, poll, queue read, queue patch, per-task transfer patch, source replacement, pause, resume, restart, remove, session save, restore, and shutdown. Evidence: `reports/api-control-summary.json`.

No daemon process remained after shutdown checks.

## Public Input Results

Apple HTTPS passed. The IPSW task advanced to `37220986` sampled bytes in `3.52` seconds, reached `47140474` bytes after resume, exposed first nonzero speed at `0.50` seconds, peaked at `2832695652` B/s, showed 8 active connections during transfer, and accepted pause and resume. Evidence: `reports/https-apple-summary.json`.

Magnet metadata passed with one API semantics bug. The KNOPPIX CD magnet resolved metadata in `3.52` seconds, exposed 9 files, reported total size `700612589`, and appeared as `paused` in the final task list. A later pause call returned `404 task_not_found` even though the task existed and was already paused. Evidence: `reports/magnet-metadata-summary.json` and `reports/api-final-summary.json`.

Torrent-file bounded download passed. The KNOPPIX DVD torrent exposed trackers and a peer, applied selected-file policy, downloaded `798567` bytes, peaked at `19013` B/s, transitioned through seeding, and accepted pause and resume. Evidence: `reports/torrent-file-summary.json`.

## Controlled Fixture Results

Local HTTP passed. A nested `downloadDir` task completed `4194304` bytes, matched SHA-256, emitted 4 bounded range headers, preserved the custom request header, and peaked at `14884993` B/s. Evidence: `reports/local-http-control-summary.json`.

CLI auth and redirect passed. Local basic auth and redirect downloads produced the expected SHA-256. Evidence: `reports/local-cli-auth-redirect-summary.json`.

Session restore passed. A second daemon restored 7 saved tasks from the same native redb session. Evidence: `reports/api-control-summary.json`.

Protocol smoke passed. `single_download` passed 24 tests, `session_smoke` passed 19 tests, `sftp_smoke` passed 3 tests, `ftp_smoke` passed 3 tests, and `ftps_smoke` passed 1 test.

## Bug Ledger

`SMOKE-006` open. Pausing an already-paused metadata-only magnet task returns `404 task_not_found`. Evidence shows the task exists in the final `/api/v1/tasks` list as `paused`. Root cause is likely invalid lifecycle transition mapping in the pause route: `pause_native_task` returns an error for `Paused -> Paused`, and `handle_pause_task` maps every error to `TaskNotFound`.

No regression was observed for bounded HTTP ranges, nested output directories, native transfer speed, WebSocket event delivery, Apple HTTPS progress, torrent tracker/peer projection, session save/restore, or daemon shutdown.

## Artifact Hygiene

Generated binaries, smoke scripts, session databases, logs, raw API payloads, and partial downloads are under `var/`, which is ignored except `var/README.md`. No matching temporary runtime artifacts were found outside `var/`.

## Next Engineering Target

Fix native task mutation error mapping and idempotent lifecycle semantics. At minimum, pausing an already-paused task should not return `task_not_found`; use a native invalid-state error or make pause idempotent.
