# Real End-to-End Smoke Report

Date: 2026-05-25
Binary: `var/releases/macos/raria`
Version: `raria 0.1.0`
Platform: macOS arm64
SHA-256: `c9fb336e97fdad2f4dc7ab2fd131919f1acaaadbbe4206fe33280443a4c9292f`
Evidence root: `var/smoke-runs/20260525-084539-real`

## Scope

This run rebuilt the release binary, started the native daemon on an ephemeral port, exercised the native HTTP JSON API, used the three maintainer-selected public inputs, sampled transfer metrics, tested lifecycle controls, saved the session, and shut the daemon down through `/api/v1/daemon/shutdown`.

The run intentionally bounded public downloads. Large artifacts were not downloaded to completion.

## API Surface

`/api/v1/health`, `/api/v1/config`, `/api/v1/stats`, `/api/v1/tasks`, `/api/v1/transfer`, `/api/v1/session/save`, and `/api/v1/daemon/shutdown` returned successful native JSON responses. The daemon reported `shuttingDown` and exited cleanly. No daemon process remained after shutdown.

Focused validation also passed:

```bash
cargo test -p raria-rpc --test native_api -- --nocapture
cargo test -p raria-cli --test native_api_smoke -- --nocapture
cargo test -p raria-cli --test session_smoke -- --nocapture
```

Results: 35 native API tests passed, 29 native daemon API smoke tests passed, and 19 session smoke tests passed.

## Public Input Results

HTTPS Apple IPSW failed to transfer data in this run. The task was accepted and exposed as running with 8 active connections, total size `238319275`, and then paused successfully. During a 12.19 second bounded sample window it stayed at `0` completed bytes and `0` bytes per second. Evidence: `reports/https-apple-summary.json`.

Magnet metadata passed. The KNOPPIX CD magnet resolved metadata, exposed 9 files, reported total size `700612589`, entered `paused` lifecycle for metadata-only mode, and accepted native BT seeding policy updates through `/api/v1/tasks/:task_id/bt/seeding`. Peers and trackers were not sampled after metadata-only pause. Evidence: `reports/magnet-metadata-summary.json`.

Torrent-file bounded download passed. The KNOPPIX DVD torrent was accepted, tracker and peer snapshots became visible, selected-file policy was applied, speed became nonzero, pause returned `paused`, and resume returned `running`. Peak sampled speed was `23754` B/s, and max sampled completed bytes was `798567`. Evidence: `reports/torrent-file-summary.json`.

## Bug Ledger

`SMOKE-001`: HTTP/HTTPS range tasks can enter `running` with active connections but never advance completed bytes. The local range fixture reproduced the issue before the public HTTPS run: running task stayed at 0 bytes with 4 active connections. The Apple HTTPS task reproduced the same zero-byte behavior with 8 active connections. This blocks ordinary HTTP/HTTPS confidence.

`SMOKE-002`: Transfer speed projection remains unreliable for ordinary HTTP/HTTPS. In this clean run the data path did not advance, so speed stayed zero. A prior smoke run also observed bytes increasing while `downloadBytesPerSecond` stayed zero. Treat speed reporting for ordinary range transfers as suspect until the range progress path is traced.

`SMOKE-003`: The real-smoke harness must treat `totalBytes: null` as valid before magnet metadata is available. The first magnet probe crashed on a null comparison, while the product task itself resolved metadata successfully. This is a harness issue, not a product failure.

## Artifact Hygiene

Generated binaries, session databases, logs, raw API payloads, and partial downloads are under `var/`, which is ignored except `var/README.md`. CLI help and completion checks wrote temporary files under `/tmp` only.

## Next Engineering Target

Investigate the ordinary HTTP/HTTPS range execution path before any new feature work. Start with `crates/raria-range/src/executor.rs`, `crates/raria-core/src/engine.rs`, `crates/raria-core/src/progress.rs`, and the native task projection used by `crates/raria-rpc/src/api.rs`. The first failing reproducer should assert that a local range-capable HTTP task advances completed bytes and exposes nonzero speed while running.
