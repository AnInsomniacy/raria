# Core Modernization Progress

This file is the compact chronological evidence trail for the raria
core-modernization tracker. Keep entries checkpoint-sized. Do not record raw
logs, packet captures, generated reports, local public-network data,
temporary downloads, local caches, API payloads, or conversation text.

Use this format:

```text
YYYY-MM-DD CM-XXX status
Changed: concise tracker, code, or behavior summary.
Verified: exact final command and result, or documentation-only reason.
Remaining: next concrete gap.
Blocked: none, or exact blocker.
```

## Current Baseline

The previous modernization run reached working multi-source failover and left
adaptive segmented transfer behavior as the next known runtime gap. raria has
working HTTP/HTTPS, FTP/FTPS, SFTP, Metalink, BitTorrent, segmented downloads,
retry, resume, native API routes, native WebSocket events, redb persistence,
structured logs, and daemon smoke tests.

The branch is not complete. Major debt remains in JSON-RPC, `Gid`, `Job`,
aria2-shaped option names, compatibility terminology, parity tests, direct
`Job` row persistence, old segment fallback tables, and old documentation.

The new tracker starts at CM-001. The first implementation-bearing checkpoint
after tracker rebuild is CM-003, Native Surface Audit.

## Log

2026-05-25 CM-001 verified
Changed: Created the active tracker under `docs/core-modernization` with a
native-only goal contract, deletion policy, library policy, restrained test
policy, roadmap, capability ledger, dependency ledger, source evidence policy,
progress log, and small checkpoint files. Removed the old docs files so
`docs` contains only the active tracker.
Verified: CSV parser validation passed for 24 files. `git diff --check`
passed. Stale old-runbook reference scan passed.
Remaining: Start CM-002 dependency policy verification.
Blocked: none.

2026-05-25 CM-001 alignment update
Changed: Aligned the tracker with aria2-next core-modernization and
libtorrent migration evidence without inheriting aria2-next compatibility
surfaces. Added explicit coverage for future native client integration, shell
completion, SCP decision, BitTorrent metadata-only behavior, duplicate
info-hash policy, PEX policy, ordinary transfer stall watchdogs, environment
proxy policy, native product docs and release closure, and final smoke
evidence.
Verified: CSV parser validation passed for 25 files. `git diff --check`
passed.
Remaining: Start CM-002 dependency policy verification.
Blocked: none.
