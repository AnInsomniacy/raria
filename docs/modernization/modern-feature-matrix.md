# Modern Feature Matrix

This matrix is the active completion contract for the raria modernization goal. aria2 is used as a feature reference only. raria public APIs, configuration, storage, and CLI must be native raria designs.

Status values:

- Covered: implemented in raria with tests or clear verification anchors.
- Partial: implemented but incomplete for the modern target.
- Gap: not implemented or still shaped by legacy compatibility.
- Excluded: intentionally out of scope as legacy or non-target behavior.

## Public Surfaces

| Capability | Modern target | raria status | Evidence | Next action |
| --- | --- | --- | --- | --- |
| Native HTTP control API | Stable `/api/v1` JSON API with raria-native resources | Partial | Daemon native smoke covers health, task listing, task creation with requested segments, pause, resume, remove, event stream, source-failure events, session save, restore, native segment resume, and `raria.toml` bearer auth; native contract tests cover config, stats, events, restart, and auth; JSON-RPC still remains | Migrate protocol-specific control tests from JSON-RPC to native API |
| Native event stream | WebSocket event schema with raria event names and typed payloads | Partial | `/api/v1/events` streams `NativeEvent` envelopes; daemon smoke verifies lifecycle and `task.source.failed` frames; aria2 notification stream still remains | Migrate remaining protocol lifecycle assertions from JSON-RPC WebSocket to native events |
| Native CLI | raria commands and fields, no aria2 compatibility naming requirement | Partial | CLI has native `--api-port` for daemon control; many options remain aria2-shaped | Redesign remaining command groups and option schema around native API fields |
| Native configuration | `raria.toml` with serde validation | Partial | CLI `--conf-path` now loads strict native TOML, maps it into runtime config including `api.auth_token_file`, and `/api/v1/config` exposes runtime projection; old parser still exists internally | Add config docs and remove old parser from public/test surface |
| Native persistence | Versioned raria schema with migrations | Partial | redb exists; native metadata, task-row, and native segment tables are initialized and tested; range checkpoint writes use native task-id segment rows; old `Gid` segment rows are read-only migration fallback with focused coverage; session save writes native task rows from job-owned opaque task ids | Remove runtime bridge/direct `Job` row reliance |
| User documentation | Modern raria docs, no compatibility claims | Partial | README now describes native `/api/v1`, event stream, and `raria.toml`; deeper docs still contain migration and compatibility wording | Continue rewriting docs around native raria model |
| Native identifier model | Opaque raria task identifiers, no aria2 GID semantics | Partial | Runtime jobs now carry opaque native `TaskId` values; registry lookup supports `TaskId`; daemon activation uses native task ids and returns a temporary executor bridge; native API creation, lookup, response projection, event projection, session save, and restart/restore preserve native ids | Move executor, cancellation, and persistence segment keys toward native `TaskId` and remove the bridge index |

## Core Runtime

| Capability | Modern target | raria status | Evidence | Next action |
| --- | --- | --- | --- | --- |
| Task model | Protocol-neutral task, file, source, segment, piece, peer, tracker, and event model | Partial | Native projections cover task summary, source, file, segment, piece, peer, tracker, event envelope, task rows, task id index, an engine native task facade, native id carried on runtime jobs, registry task-id lookup, and native executor-facing helpers; `Job` still drives runtime | Move engine ownership from migration `Job`/`Gid` to native task objects |
| Queue scheduling | Waiting, active, paused, stopped, seeding, completion, and bounded concurrency | Partial | Scheduler and status machine exist; scheduler queue storage is keyed by native `TaskId`; legacy `Gid` queue methods remain as migration adapters; engine exposes native task activation handles; daemon activation uses native task ids | Add native priorities and remove legacy queue adapters when JSON-RPC is retired |
| Pause, resume, remove | Immediate and graceful lifecycle transitions across protocols | Partial | Native API exposes pause, resume, remove, and restart through the engine native task facade; daemon shutdown cancels active work through a native task operation | Move controls into native task service and `TaskId` ownership |
| Progress and global stats | Accurate per-task and global speed, completed bytes, active connections | Partial | `/api/v1/stats` exposes native task counts and speed fields; runtime progress still comes through old event bus | Extend native stats and event stream coverage |
| Runtime options | Safe runtime mutation of limits, queue position, sources, and task settings | Partial | Native source read API exists; mutation still remains aria2-shaped | Redesign typed runtime mutation API |
| Structured logs | Durable operational logs with redaction and correlation IDs | Covered | `logging-contract.md`, structured logging helpers and tests | Keep and expand during API migration |
| Hooks | Modern lifecycle hooks or event consumers | Partial | start, complete, error hooks exist | Decide native hook model after event schema |

## HTTP, HTTPS, FTP, FTPS, and SFTP

| Capability | Modern target | raria status | Evidence | Next action |
| --- | --- | --- | --- | --- |
| HTTP/HTTPS downloads | reqwest-backed downloads with redirects, headers, auth, TLS, mTLS, cookies, proxy, and range support | Partial | `raria-http`, `single_download`, `http_config_smoke` | Re-anchor options to native config and API |
| FTP/FTPS downloads | suppaftp-backed FTP and explicit FTPS with resume and proxy support | Partial | `raria-ftp`, FTP and FTPS smoke tests | Re-anchor options and complete matrix cases |
| SFTP downloads | russh-backed SFTP with password, key auth, known_hosts, and proxy support | Partial | `raria-sftp`, SFTP smoke tests | Re-anchor options and complete matrix cases |
| Multi-source downloads | Multiple protocol sources for one task with failover and health scoring | Partial | URI registry and mirror failover exist | Add health scoring and native source model |
| Adaptive segmented downloads | Dynamic segment planning, retry, cancellation, checkpointing, and limiter integration | Partial | `raria-range::SegmentExecutor` | Add adaptive behavior beyond static planning |
| Resume and crash recovery | Robust resume across daemon restart and interrupted segments | Partial | native task-id segment checkpoint writes/reads, read-only old `Gid` segment fallback, session tests, native session save, native restart/restore smoke, daemon native segment resume smoke, and focused fallback migration coverage exist | Remove old `Gid` segment table after native schema migration coverage |
| Conditional requests | Conditional GET and resource-change handling | Partial | conditional GET and If-Range tests exist | Revalidate under native task model |
| Whole-file checksum | Multiple algorithms and terminal verification | Covered | `checksum.rs`, daemon verification path | Keep and expose natively |
| Piece checksum | Piece-level verification from Metalink and torrent metadata | Partial | Metalink piece checksum wiring and BT metadata | Unify piece model across range and BT |
| Remote filename and metadata | Content-Disposition, remote timestamps, content type | Partial | Content-Disposition exists; remote-time is not fully covered | Add native metadata model |
| Cookie and netrc | Cookie load/save and netrc credential lookup | Partial | HTTP cookies and netrc support exist | Move into typed auth config |
| File allocation and disk strategy | none, preallocation, truncation, fallocate where supported | Partial | `file_alloc.rs`, executor allocation tests | Revalidate with native storage and resume |

## Metalink

| Capability | Modern target | raria status | Evidence | Next action |
| --- | --- | --- | --- | --- |
| Metalink v3 and v4 parsing | Parse modern Metalink inputs and normalize into native tasks | Partial | `raria-metalink` parser and normalizer | Expand source-derived test coverage |
| Mirror priority and filters | Location, protocol preference, priority, and unique protocol handling | Partial | aria2 source has rich filtering; raria has priority handling | Complete filters in native normalizer |
| Checksums | Whole-file and piece checksums | Partial | Normalizer carries checksum fields | Add full verification anchors |
| Multi-file Metalink | Multiple files become native task graph | Partial | RPC dispatch creates multiple jobs | Replace relation fields with native graph model |
| Metaurl and torrent integration | Metalink torrent metaurl and hybrid downloads | Gap | aria2 has metaurl grouping; raria support is limited | Design native source graph |

## BitTorrent

| Capability | Modern target | raria status | Evidence | Next action |
| --- | --- | --- | --- | --- |
| Torrent file download | librqbit-backed torrent file ingestion and download | Partial | `raria-bt::BtSource::TorrentBytes/File`, smoke tests | Re-anchor in native task model |
| Magnet metadata | Magnet ingestion and metadata resolution | Partial | librqbit magnet path exists | Add explicit metadata lifecycle tests |
| DHT | DHT enablement and persistence | Partial | DHT persistence tests exist | Verify full behavior against librqbit docs and runtime tests |
| UDP tracker | UDP tracker support if provided by librqbit or a small native layer | Gap | raria tests focus on HTTP tracker path | Verify library support and implement if missing |
| PEX | Peer exchange if supported or reasonably implementable | Gap | aria2 has UTPex tests; raria does not expose PEX capability | Verify librqbit support and decide implementation |
| WebSeed | BEP-17/BEP-19 WebSeed support | Partial | `webseed.rs` and gap ledger tests exist | Integrate into native BT lifecycle |
| File selection | Select files before download and expose file progress | Partial | `only_files`, `BtFileInfo`, RPC option support | Move to native task file model |
| Tracker management | Add, exclude, timeout, interval, and status projection | Partial | `NativeTrackerSnapshot` exists; tracker control and status API are not native yet | Add native tracker API and runtime wiring |
| Peer projection | Expose peer list, speeds, and seeder status | Partial | `NativePeerSnapshot` exists; runtime API still exposes aria2-shaped peer projection | Wire native peer snapshots into `/api/v1` |
| Seeding controls | ratio, time, stop timeout, seed-only lifecycle | Partial | seed ratio/time logic exists | Revalidate and expose natively |
| Fastresume | Persist and restore BT progress | Partial | librqbit fastresume enabled and tested | Tie into versioned raria persistence |
| BT encryption | Modern transport policy only if supported by mature library | Excluded for MSE/ARC4 | Goal excludes MSE/ARC4 | Remove misleading config if unsupported |
| LPD | Local peer discovery | Excluded | Goal excludes LPD | Remove from matrix once no code references remain |

## Advanced Downloader Behavior

| Capability | Modern target | raria status | Evidence | Next action |
| --- | --- | --- | --- | --- |
| Rate limiting | Global and per-task download/upload limits | Partial | limiter and runtime mutation tests exist | Complete native API and upload limits |
| Retry policy | Typed transient/permanent classification and retry controls | Partial | executor retry and daemon classification exist | Move to typed error model |
| Mirror health | Server statistics and adaptive source selection | Partial | Native daemon event smoke covers source failover reporting; adaptive source scoring is not implemented | Implement source health scoring and native mirror stats |
| File conflict policy | overwrite, auto-rename, partial reuse, selected-file cleanup | Partial | rename and overwrite options exist | Move to native file policy |
| Disk cache and mmap | Only if justified by measurable modern value | Gap | aria2 has disk cache and mmap options | Evaluate after core model migration |
| DNS and interface selection | Async DNS, bind interface, multiple interface, IPv6 controls | Gap | aria2 exposes these; raria does not model them fully | Verify library support and modern need |
| Process lifecycle | stop timer, stop with process, graceful signal handling | Partial | daemon handles Ctrl-C, SIGTERM, SIGUSR1 | Add native shutdown policy |
| Security and redaction | Auth, token, TLS, secret redaction, path safety | Partial | Native bearer auth covers protected native API routes including stats; daemon smoke verifies `raria.toml` token-file auth on `/api/v1`; RPC secret and old token-in-params path still remain | Remove JSON-RPC auth surface and add native secret redaction coverage |

## Architecture Decisions

| Decision | Status | Evidence | Next action |
| --- | --- | --- | --- |
| Preserve mature protocol libraries | Accepted | reqwest, suppaftp, russh, quick-xml, librqbit, redb, and axum verified against public docs | Keep verifying exact API behavior at each implementation slice |
| Use `/api/v1` resource API | Partial | Native health, config, task, stats, event, task-control contract tests pass; daemon smoke covers core lifecycle routes | Add daemon event smoke coverage and replace JSON-RPC as the daemon control surface |
| Use `/api/v1/events` WebSocket stream | Partial | Native WebSocket contract and daemon smoke tests pass for lifecycle, progress, and source-failure events | Migrate remaining protocol lifecycle assertions from JSON-RPC WebSocket to native stream |
| Use `raria.toml` strict schema | Partial | Native schema, runtime conversion including API bearer token loading, CLI loading, daemon auth smoke, and `/api/v1/config` projection tests pass | Expand schema coverage and remove old config parser from public flow |
| Use versioned redb schemas | Partial | `NativeStoreMetadata` and `NativeTaskRow` DTOs exist; redb store initializes native metadata and task-row tables; save writes native rows; restore reads native rows first | Add ID indexes, migration fixtures, and remove direct `Job` row reliance |

## Excluded Legacy Features

| Capability | Reason |
| --- | --- |
| XML-RPC | Legacy control surface, explicitly out of scope |
| libaria2 C API compatibility | Legacy embedding surface, explicitly out of scope |
| aria2 session/control-file compatibility | Legacy storage compatibility, explicitly out of scope |
| HTTP/1.1 pipelining | Historical HTTP behavior, explicitly out of scope |
| BitTorrent MSE/ARC4 | Legacy encryption, explicitly out of scope |
| LPD | Legacy local peer discovery, explicitly out of scope |
| aria2 config syntax compatibility | Replaced by `raria.toml` |
| Historical packaging and platform baggage | Non-core modernization target |
