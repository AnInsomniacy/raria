# Real End-to-End Smoke Report

Date: 2026-05-25
Binary: `var/releases/macos/raria`
Version: `raria 0.1.0`
Platform: macOS arm64
SHA-256: `ef682fb2b31e2e7a59e6ad6e2bac174fe0675cc8fd9d3ffc9974c832ffbb8d81`
Evidence root: `var/smoke-runs/20260525-091956-fixed-clean`

## Scope

This run rebuilt the release binary, started the native daemon on an ephemeral port, exercised the native HTTP JSON API, used the three maintainer-selected public inputs, sampled transfer metrics, tested lifecycle controls, saved the session, and shut the daemon down through `/api/v1/daemon/shutdown`.

The run intentionally bounded public downloads. Large artifacts were not downloaded to completion.

## API Surface

`/api/v1/health`, `/api/v1/config`, `/api/v1/stats`, `/api/v1/tasks`, `/api/v1/transfer`, `/api/v1/session/save`, and `/api/v1/daemon/shutdown` returned successful native JSON responses. The daemon reported `shuttingDown` and exited cleanly. No daemon process remained after shutdown.

Focused validation also passed:

```bash
cargo test -p raria-core update_progress_projects_speed_after_delta
cargo test -p raria-range multi_segment_passes_bounded_lengths_to_backend
cargo test -p raria-cli --test native_api_smoke daemon_native_http_range_task_progresses_with_bounded_segments
```

## Public Input Results

Local HTTP control passed. A nested `downloadDir` task completed `4194304` bytes, produced the expected SHA-256, exposed nonzero speed, and issued bounded segment ranges: `bytes=0-1048575`, `bytes=1048576-2097151`, `bytes=2097152-3145727`, and `bytes=3145728-4194303`. Peak sampled speed was `8153271` B/s. Evidence: `reports/local-http-control-summary.json`.

HTTPS Apple IPSW passed the bounded sample. The task accepted the public URL, created nested output directories, advanced from zero to `36593706` sampled bytes in `3.06` seconds, exposed first nonzero speed at `0.51` seconds, peaked at `614392320` B/s, and paused successfully. Evidence: `reports/https-apple-summary.json`.

Magnet metadata passed. The KNOPPIX CD magnet resolved metadata in `4.10` seconds, exposed 9 files, reported total size `700612589`, entered `paused` lifecycle for metadata-only mode, and accepted native BT seeding policy updates through `/api/v1/tasks/:task_id/bt/seeding`. Peers and trackers were not sampled after metadata-only pause. Evidence: `reports/magnet-metadata-summary.json`.

Torrent-file bounded download passed. The KNOPPIX DVD torrent was accepted, tracker and peer snapshots became visible, selected-file policy was applied, speed became nonzero, pause returned `paused`, and resume returned `running`. Peak sampled speed was `7782` B/s, and max sampled completed bytes was `798567`. Evidence: `reports/torrent-file-summary.json`.

## Bug Ledger

`SMOKE-001` fixed. HTTP/HTTPS native range tasks now pass bounded segment lengths to the backend. HTTP emits `Range: bytes=start-end` when segment length is known. The executor still caps reads locally.

`SMOKE-002` fixed. Native range progress now updates `downloadBytesPerSecond` from real byte deltas. The API exposes nonzero speed during local HTTP and public HTTPS transfers.

`SMOKE-003` fixed in the smoke harness. Magnet metadata probes now treat `totalBytes: null` as valid before metadata resolution.

`SMOKE-004` fixed. Native range tasks now create nested output directories before opening files.

`SMOKE-005` fixed. Spawned range task failures now transition the native task to `failed` instead of leaving it stuck as `running`.

## Artifact Hygiene

Generated binaries, session databases, logs, raw API payloads, and partial downloads are under `var/`, which is ignored except `var/README.md`. CLI help and completion checks wrote temporary files under `/tmp` only.

## Next Engineering Target

Continue from the active `docs/core-modernization/roadmap.csv` checkpoint. The HTTP/HTTPS smoke blocker is closed and should remain covered by focused native API, range executor, and engine speed regression tests.
