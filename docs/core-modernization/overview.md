# raria Core Modernization

This tracker is the authoritative execution system for finishing raria as a
modern Rust-native download manager. It replaces the old modernization runbook
model. raria does not preserve aria2 public APIs, option names, configuration
syntax, storage formats, identifiers, tests, docs, or ecosystem compatibility.

The main modernization reference is the aria2-next maintenance body,
especially the core-modernization and libtorrent migration trackers. Use it
for deletion discipline, library-first ownership, checkpoint sizing, focused
verification, storage truth, stale-surface cleanup, and final smoke discipline.
Do not copy aria2-next's C++ stack or retained product surfaces. raria's public
contract is native.

## Tracker Files

| Path | Role |
| --- | --- |
| `overview.md` | Scope, policy, verification, update rules, and active authority |
| `roadmap.csv` | Single checkpoint index and progress entry point |
| `capability-ledger.csv` | Feature ownership, status, and closing checkpoint |
| `dependency-ledger.csv` | Rust library ownership and replacement decisions |
| `source-evidence.md` | Source inputs, local evidence rules, and old-doc migration notes |
| `progress.md` | Compact chronological evidence trail |
| `checkpoints/CM-001-tracker-rebuild.csv` | Tracker rebuild, old-doc retirement, and validation |
| `checkpoints/CM-002-dependency-policy.csv` | Rust library ownership and obsolete dependency decisions |
| `checkpoints/CM-003-native-surface-audit.csv` | Native API, events, CLI, config, docs, and legacy-surface inventory |
| `checkpoints/CM-004-core-ownership-audit.csv` | TaskId, Job, Gid, scheduler, event, and persistence ownership audit |
| `checkpoints/CM-005-native-api-events.csv` | Native API and event stream closure |
| `checkpoints/CM-006-native-cli-config.csv` | Native CLI and `raria.toml` closure |
| `checkpoints/CM-007-task-identity.csv` | Public and internal TaskId ownership |
| `checkpoints/CM-008-native-task-runtime.csv` | Task runtime model and lifecycle service |
| `checkpoints/CM-009-native-persistence.csv` | Versioned redb schema and old row removal |
| `checkpoints/CM-010-session-recovery.csv` | Crash recovery, session save, and restore |
| `checkpoints/CM-011-http-contract.csv` | HTTP and HTTPS native transfer closure |
| `checkpoints/CM-012-ftp-sftp-contract.csv` | FTP, FTPS, and SFTP native transfer closure |
| `checkpoints/CM-013-multi-source-adaptive.csv` | Mirror health, failover, adaptive segments, and resume safety |
| `checkpoints/CM-014-integrity-storage.csv` | Checksums, piece state, allocation, and conflict policy |
| `checkpoints/CM-015-metalink-native.csv` | Retained modern Metalink behavior and old XML baggage removal |
| `checkpoints/CM-016-bittorrent-ingress.csv` | Torrent, magnet, DHT, tracker, and metadata ingress |
| `checkpoints/CM-017-bittorrent-runtime.csv` | WebSeed, file selection, peers, seeding, limits, and fastresume |
| `checkpoints/CM-018-transfer-policy.csv` | Rate limits, retry classification, DNS/interface policy |
| `checkpoints/CM-019-daemon-security-logs.csv` | Daemon lifecycle, hooks, auth, redaction, and structured logs |
| `checkpoints/CM-020-legacy-deletion.csv` | JSON-RPC, parity tests, compatibility docs, and stale options removal |
| `checkpoints/CM-021-product-docs-release.csv` | Native client contract, completion, product docs, and release closure |
| `checkpoints/CM-022-final-validation.csv` | Final workspace validation and tracker closure |

Read `overview.md` and `roadmap.csv` first after every resume or context
compaction. During implementation, read only the active checkpoint file plus
the ledgers needed for the current decision. Read the full tracker only for
final review or when a blocker crosses checkpoint boundaries.

## Goal Contract

Finish raria as a modern Rust-native download manager and daemon engine.
Public surfaces are `raria.toml`, `/api/v1` HTTP JSON resources,
`/api/v1/events` WebSocket events, versioned native persistence schemas,
native CLI names, opaque task identifiers, structured logs, shell completion,
and native documentation.

The implementation must remove legacy compatibility instead of preserving it.
Delete JSON-RPC, aria2 method names, aria2 option names, Gid-facing public
behavior, aria2 config syntax, aria2 session/control-file compatibility,
AriaNg/Motrix compatibility, parity tests, compatibility docs, and migration
adapters when useful behavior has native coverage.

raria may be used by a future Motrix Next adapter, but the adapter must speak
raria's native API and event stream. raria must not shape its API, task state,
field names, identifiers, persistence, errors, or events around aria2-next,
Motrix legacy adapters, AriaNg, or JSON-RPC.

Stop only when every checkpoint in `roadmap.csv` is verified, both ledgers
record final ownership, stale-surface scans pass, and the full Rust validation
ladder passes.

## Library Policy

The policy is library-first, not library-blind. Use mature Rust libraries where
they own protocol correctness, maintenance, security, and performance better
than local code can. Keep local code only where raria owns a real product
boundary or where no suitable maintained library exposes the needed behavior.

| Domain | Target owner | Policy |
| --- | --- | --- |
| Runtime | Tokio | Keep Tokio as the single async runtime |
| HTTP and HTTPS | reqwest | Use reqwest for redirects, headers, auth, proxy, TLS, mTLS, cookies, range, and streaming |
| FTP and FTPS | suppaftp | Use suppaftp with focused smoke coverage for auth, resume, FTPS, and proxy behavior |
| SFTP | russh and russh-sftp | Use async SSH/SFTP libraries with focused known_hosts, password, key, proxy, and resume coverage |
| Native API and events | axum | Use axum for resource routes and WebSocket event stream |
| JSON | serde_json | Keep JSON serialization through serde-owned types |
| Metalink | quick-xml | Retain only modern Metalink value; remove old XML compatibility baggage |
| BitTorrent | librqbit | Use librqbit for torrent, magnet, DHT, tracker, peer, piece, and fastresume behavior where public APIs support it |
| Persistence | redb | Use redb as the embedded engine, not as the schema model |
| TLS | rustls | Prefer rustls unless a concrete dependency forces another boundary |
| Rate limiting | governor plus raria policy | Keep small native policy layers around mature primitives |
| CLI and completion | clap and clap_complete | Use native command names and generated completion only for retained commands |
| Logging | tracing | Keep structured logs with native task correlation and redaction |

If a target library cannot cover required modern behavior, implement the
smallest robust raria-native layer with focused tests and record the decision
in `dependency-ledger.csv`.

## Scope

In scope:

| Area | Expected outcome |
| --- | --- |
| Native public surface | API, event stream, CLI, config, docs, IDs, and logs use raria-owned names |
| Core runtime | Task lifecycle, scheduling, cancellation, mutation, and event ownership move to native tasks |
| Persistence | Versioned native schemas replace direct `Job` rows and old `Gid` segment fallback |
| Ordinary transfers | HTTP, HTTPS, FTP, FTPS, and SFTP use mature libraries with raria-native policy |
| SSH-family transfers | SFTP is required; SCP must be implemented only if a mature Rust path is verified, otherwise documented as a technical limitation |
| Multi-source transfers | Source health, failover, adaptive segments, resume, and verified completion are native |
| Integrity and disk | Whole-file checksums, piece checksums, allocation, and conflict policy are verified |
| Metalink | Keep only modern manifest value that feeds native task graphs |
| BitTorrent | Finish librqbit-backed torrent, magnet, metadata-only, DHT, UDP trackers, PEX where available, WebSeed, file selection, duplicate info-hash policy, seeding, and fastresume behavior |
| Network policy | Proxy, environment proxy, DNS, IPv6, interface binding, no-progress watchdogs, and retry classification are implemented through selected libraries or recorded as technical limitations |
| Native client integration | A stable resource API and WebSocket stream are sufficient for future Motrix Next integration after Motrix adapts to raria |
| Product closure | README, completion, release notes, packaging metadata, examples, and validation fixtures describe only the native product |
| Cleanup | Delete stale options, old tests, old docs, compatibility adapters, and legacy public surfaces |

Out of scope:

| Area | Decision |
| --- | --- |
| XML-RPC | Delete |
| JSON-RPC as final public surface | Delete |
| libaria2 C API compatibility | Delete |
| aria2 session or control-file compatibility | Delete |
| aria2 config syntax and option names | Delete |
| AriaNg or Motrix compatibility | Delete |
| HTTP pipelining | Delete |
| BitTorrent MSE or ARC4 | Delete |
| LPD | Delete |
| ED2K | Delete from the raria target |
| aria2 JSON-RPC compatibility for Motrix Next | Delete; future clients must adapt to raria-native resources and events |
| aria2-style URI parameter expansion | Delete unless replaced by an explicit native batch task format |
| SCP | Implement only if mature Rust support is proven; otherwise document as a technical limitation and prefer SFTP |
| Historical packaging or platform baggage | Delete unless needed by a current supported Rust build |
| Browser-cookie import | Delete unless a later checkpoint proves modern value |

## Removal Policy

Delete obsolete code once native behavior is covered or the feature is pruned.
Do not leave inactive modes, hidden fallback engines, compatibility options,
or tests that imply removed behavior still exists.

Expected removal targets include JSON-RPC routes and methods, aria2-shaped
status fields, `Gid` public behavior, parity tests, compatibility docs, stale
CLI option names, old configuration aliases, direct `Job` row persistence, old
segment tables, legacy event projections, migration adapters, old Metalink/XML
baggage, unsupported BT policy shims, legacy Motrix or AriaNg adapter claims,
and obsolete test fixtures.

Keep only small native helpers that protect supported behavior and do not
duplicate mature library responsibility.

## Storage Truth Policy

Completion must be based on verified transfer, disk flush, and integrity state.
User-facing progress may include in-flight bytes when useful, but terminal
completion, session save/restore decisions, cleanup, and API `completed`
lifecycle must not depend on unverified bytes.

Range transfers, Metalink piece checks, and BitTorrent verified state must
converge on a native piece and file truth model before final completion is
claimed.

## Test Policy

Tests must be restrained and high value. Add or retain tests for native public
contracts, protocol boundaries, persistence migrations, completion truth,
resume, confirmed regressions, stale-surface scans, and security-sensitive
behavior.

Delete tests that cover compatibility behavior, old protocol internals,
incidental logs, broad parity, removed options, hidden fallback paths, or
client-specific legacy adapters. Do not add broad scaffolding when a focused
unit, contract, or smoke test proves the behavior.

## Commit Policy

Work on the current branch only. Do not push. Commit only meaningful verified
boundaries such as a completed tracker checkpoint, a native surface replacement,
a protocol/runtime closure, or a major legacy deletion. Commit messages must
be concise professional English.

## Verification Policy

Use focused validation during checkpoints. Run the smallest relevant command
for the active change and record it in the checkpoint file and `progress.md`.
Run `cargo check --workspace --locked` after meaningful integration slices.

Final validation requires:

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Use maintainer-selected torrent fixtures only for targeted BitTorrent smoke
validation. Do not commit temporary downloads, generated logs, packet captures,
network scratch data, local caches, or raw API payloads.

Final smoke evidence should cover ordinary HTTPS progress or completion,
range resume, native API task creation and events, BitTorrent torrent-file
ingress, BitTorrent magnet metadata where practical, and one native persistence
restore path. Public network evidence is useful final evidence, not a unit-test
gate.

## Update Rules

Before every checkpoint, update the active checkpoint row or file with the
target and expected validation. After every checkpoint, update `roadmap.csv`,
the matching checkpoint file, `progress.md`, and any ledger whose ownership
decision changed.

Keep entries checkpoint-sized and durable. Do not record conversation text.
Do not maintain multiple active progress sources.
