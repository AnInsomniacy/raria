# Native ED2K/eMule Workstream

This tracker is the active evidence system for native ED2K/eMule support in
raria. It replaces the earlier core-modernization exclusion for ED2K. The
target is a Rust-native protocol backend with raria-native public surfaces,
not aria2 compatibility and not a translated GPL code port.

## Tracker Files

| Path | Role |
| --- | --- |
| `overview.md` | Scope, authority, architecture, verification, and update rules |
| `roadmap.csv` | Single checkpoint index and progress entry point |
| `capability-ledger.csv` | ED2K/eMule capability decisions and closing checkpoint |
| `dependency-ledger.csv` | Rust library ownership and rejected dependency directions |
| `progress.md` | Compact chronological evidence trail |
| `checkpoints/*.csv` | Checkpoint-sized acceptance and validation records |

Read `overview.md` and `roadmap.csv` first after every resume or context
compaction. During implementation, read only the active checkpoint file plus
the ledgers needed for the current decision. Read the full tracker only for
final review or when a blocker crosses checkpoint boundaries.

## Authority

The primary behavior reference is `/Users/sekiro/Projects/oss/amule`.
Relevant aMule owners include `ED2KLink`, `Server`, `ServerList`,
`ServerSocket`, `ServerUDPSocket`, `ClientTCPSocket`, `ClientUDPSocket`,
`DownloadClient`, `DownloadQueue`, `PartFile`, `KnownFile`, `SharedFileList`,
`UploadQueue`, `ClientCredits`, `SearchList`, and `src/kademlia`.

The engineering reference is `/Users/sekiro/Projects/personal/aria2-next`,
especially `docs/maintenance/ed2k-refactor` and
`docs/maintenance/ed2k-download-hardening`. Use it for checkpoint sizing,
capability grouping, prune decisions, and verification discipline. Do not copy
aria2-next code or inherit its JSON-RPC, aria2 option, session, Motrix, C++,
or packaging surfaces.

aMule and aria2-next are GPL-family references. raria is Apache-2.0. Use those
trees for behavior research only. Implement protocol behavior independently in
Rust with raria-owned data structures, packet codecs, state machines, tests,
and public contracts.

## Product Contract

ED2K support must be exposed through raria-native surfaces:

| Surface | Target |
| --- | --- |
| Configuration | `raria.toml` `[ed2k]` section with native field names |
| Task creation | `POST /api/v1/tasks` with ED2K sources and native ED2K options |
| Task state | `GET /api/v1/tasks/{taskId}` with an `ed2k` status object |
| Search | Native `/api/v1/ed2k/searches` resources |
| Events | `/api/v1/events` messages with `task.ed2k.*` status names |
| CLI | Natural native CLI behavior for ED2K links and concise status output |
| Persistence | Versioned redb schemas owned by raria |
| Logs | Structured tracing fields with native task correlation |

Do not add JSON-RPC, aria2 method names, aria2 option names, Gid-facing public
behavior, aria2 session or control-file compatibility, aMule database import,
AriaNg compatibility, legacy Motrix adapters, or migration shims.

## Rust Library Policy

No mature Rust crate currently owns a complete ED2K/eMule downloader engine.
The library strategy is to use mature primitives and keep protocol ownership
inside `crates/raria-ed2k`.

| Domain | Owner policy |
| --- | --- |
| Async runtime and sockets | Tokio TCP, UDP, timers, and task coordination |
| Persistence | redb with versioned raria schemas |
| Serialization | serde for native API and persistence rows |
| ED2K root hash and hashsets | RustCrypto `md4` plus raria ED2K hashset rules |
| AICH | raria SHA-1 tree implementation using mature hash primitives |
| Compression | flate2 for compressed ED2K part payloads |
| Rate policy | governor plus small raria scheduling policy |
| API and events | axum and the existing native event model |
| Logging | tracing with redaction and task correlation |

Generic Kademlia crates are not eMule Kad implementations. ED2K hash crates
are not download engines. They may be used only after the active checkpoint
proves a narrow benefit, compatible license, and stable maintenance surface.

## Architecture

Add `crates/raria-ed2k` as a stateful P2P backend. Do not absorb ED2K into
`raria-range`; ED2K is not a byte-range mirror protocol. The backend should
own packet codecs, link parsing, identity, source discovery, peer lifecycle,
part transfer, Kad, sharing, upload queue, credits, search, and ED2K-specific
persistence rows.

`crates/raria-core` remains the task, scheduling, cancellation, persistence,
checksum, lifecycle, and native event owner. It should gain `JobKind::Ed2k`,
`SourceProtocol::Ed2k`, native ED2K task state projections, ED2K persistence
row integration, and task lifecycle hooks without exposing legacy identifiers.

`crates/raria-rpc` owns native HTTP resources and WebSocket events. It should
expose ED2K as typed native resources, not as a compatibility method set.

`crates/raria-cli` owns daemon activation, command parsing, and concise native
CLI output. It should route ED2K tasks to the ED2K backend and keep local
generated smoke output under `var/`.

Current completion boundary: ED2K protocol primitives, native task projection,
events, search resources, persistence rows, runtime context, bounded scheduler
status ticks, local-socket server TCP/UDP exchange ownership, bounded
local-socket Kad UDP source, keyword, publish, firewall, and timeout handling,
bounded local-socket peer TCP handshake, queue, source exchange, part request,
payload validation, timeout handling, local file disk completion, native resume
snapshot restore, sharing-on-completion, verified upload serving, UDP reask
handling, daemon inline peer transfer to verified disk completion, daemon
server and Kad discovery scheduling, native search execution, daemon upload
listeners, credit persistence, local smoke evidence, and final validation are
verified. Public ED2K network smoke remains manual evidence, not an automated
gate.

## Capability Scope

In scope:

| Area | Expected outcome |
| --- | --- |
| ED2K links | File links, safe names, source metadata, AICH metadata, and search links parse into native task/search models |
| File identity | ED2K root hash, part hashsets, AICH root/tree, file size, and large-file handling are represented truthfully |
| Metadata files | `server.met` and `nodes.dat` are parsed only as useful network bootstrap data, not as compatibility storage |
| Identity | Stable ED2K client identity persists through raria-native state |
| Server TCP and UDP | Login, status, source discovery, callbacks, and server metadata follow useful aMule behavior |
| HighID and LowID | Callback state and firewalled boundaries are explicit and do not poison direct scheduling |
| Peer sessions | Hello, capability truth, file status, hashset, queue rank, request flow, and failure state are implemented |
| Source Exchange | SX1/SX2 useful source discovery is retained with safe dedupe and retry policy |
| Transfer | Normal, I64, compressed parts, cancellation, retry, integrity, disk truth, and resume are implemented |
| Kad | Bootstrap, routing, source search, source publish, keyword search, refresh, and firewall state are implemented |
| Sharing | Completed or imported files can be shared through native metadata where enabled |
| Upload and credits | Upload queue, UDP reask responses, ranks, slots, and practical credit counters are implemented truthfully |
| Search | Server and Kad search expose native search resources and startable ED2K links |
| Integration | API, events, CLI, config, docs, persistence, logs, and final validation match raria-native contracts |

Prune:

| Area | Decision |
| --- | --- |
| aMule GUI, web UI, text client, remote GUI | Delete |
| Chat, friends, comments, ratings, preview, collections, captcha | Delete |
| Old part.met, known.met, known2.met, clients.met import | Delete unless a later maintainer request creates a migration goal |
| Router helper stacks and automatic UPnP/NAT control | Delete; expose explicit listen/firewall configuration only |
| ED2K crypt or secure-ident claims without full send/receive/state/failure ownership | Keep unadvertised until complete |
| aria2 JSON-RPC, aria2 options, Motrix legacy fields | Delete |

## Test Policy

Use restrained high-value tests. Add tests for parser contracts, packet
framing, state transitions, persistence rows, integrity, disk completion truth,
native API contracts, event names, security-sensitive behavior, and confirmed
regressions.

Do not add public-network tests as automated gates. Do not add ignored
placeholders, socket-heavy scaffolding, broad parity tests, compatibility-only
tests, or tests that only mirror implementation details. Public ED2K downloads
are manual smoke evidence recorded compactly after local validation passes.

## Verification Policy

Use the smallest relevant command for each checkpoint and record it in the
active checkpoint file and `progress.md`. Run `cargo check --workspace --locked`
after meaningful integration slices.

Final validation requires:

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Manual public ED2K smoke should record exact inputs, observed source discovery,
queue state, transfer progress, limitations, and cleanup under `var/` with
only concise conclusions committed.

## Update Rules

Before every checkpoint, update the active checkpoint file with the target and
expected validation. After every checkpoint, update `roadmap.csv`, the matching
checkpoint file, `capability-ledger.csv` or `dependency-ledger.csv` when a
decision changes, and `progress.md`.

Keep records compact and durable. Do not commit raw packet captures, public
network logs, downloaded payloads, generated release output, temporary API
payloads, local caches, or conversation text.
