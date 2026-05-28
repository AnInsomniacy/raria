# Native ED2K/eMule Progress

This file is the compact chronological evidence trail for
`docs/core-modernization/ed2k-native`.

## 2026-05-28 ED2K-001 verified

Changed: Created the native ED2K/eMule tracker under
`docs/core-modernization/ed2k-native`. The tracker defines aMule as the primary
behavior reference, aria2-next ED2K trackers as engineering references, and
raria-native public surfaces as the only accepted product contract. It records
the Rust library strategy, GPL isolation rule, retained downloader scope,
pruned application-shell behavior, restrained test policy, and 26 checkpoint
execution roadmap.

Verified: CSV validation passed for 54 tracker files. Stale ED2K exclusion
phrase scan passed. `git diff --check` passed. `cargo check --workspace
--locked` passed.

Remaining: Start ED2K-002 authority, license, and dependency audit.

Blocked: none.

## 2026-05-28 ED2K-002 verified

Changed: Recorded the authority, license, and dependency audit before any ED2K
implementation work. The tracker now maps the relevant aMule behavior owners,
the aria2-next ED2K refactor and hardening ledgers, current Rust dependency
candidates, rejected generic DHT directions, and the GPL-to-Apache isolation
rule.

Verified: Local source inspection covered aMule ED2K links, server state,
server TCP/UDP, peer TCP/UDP, download queue, part file, known/shared files,
upload queue, credits, search, dead sources, protocol constants, and Kad
owners. `cargo search ed2k` and `cargo search emule` found no complete Rust
ED2K/eMule downloader engine. `cargo info` verified the current scope of
`ed2k`, `md4`, `flate2`, `kademlia-dht`, `kad`, and `libp2p-kad`.

Remaining: Start ED2K-003 crate boundary and native model.

Blocked: none.

## 2026-05-28 ED2K-003 verified

Changed: Added the native ED2K crate and core model boundary without runtime
protocol behavior. `crates/raria-ed2k` now exists with ownership modules for
hash, identity, Kad, link parsing, peer state, persistence, server handling,
sharing, and transfer planning. `raria-core` now has `JobKind::Ed2k`,
`SourceProtocol::Ed2k`, a strict `[ed2k]` native config section, the
`ed2k_identities` redb table, and `NativeEd2kIdentityRow` schema versioning.
ED2K links are classified as ED2K jobs at the core boundary, and daemon
activation fails them explicitly until the runtime backend exists.

Verified: The RED checks failed before implementation because `raria-ed2k`,
`JobKind::Ed2k`, `SourceProtocol::Ed2k`, `[ed2k]`, and ED2K identity
persistence did not exist. After implementation, `cargo check -p raria-ed2k
--locked`, focused native model tests, `cargo test -p raria-core --test
native_config --locked`, and the ED2K identity persistence roundtrip passed.

Remaining: Start ED2K-004 link parser and file identity.

Blocked: none.

## 2026-05-28 ED2K-004 verified

Changed: Added native ED2K link parsing in `raria-ed2k`. File links now parse
safe names, sizes, root hashes, part hashes, AICH roots, inline sources, crypt
options, and source client hashes into native Rust structs. Server, serverlist,
nodeslist, and search links parse as typed metadata models. Native task
creation through `/api/v1/tasks` now classifies ED2K file links as ED2K jobs
with opaque task IDs.

Verified: `cargo test -p raria-ed2k link --locked` passed for file metadata,
metadata links, safe names, and malformed inputs. `cargo test -p raria-rpc
--test native_api task_creation_ed2k_source_uses_ed2k_backend --locked` passed.

Remaining: Start ED2K-005 hashset and AICH primitives.

Blocked: none.

## 2026-05-28 ED2K-005 verified

Changed: Added ED2K integrity primitives in `raria-ed2k`. The hash module now
owns md4-backed ED2K digests, ED2K root hash construction, theoretical
part-hash counts, provided hashset validation, SHA-1 AICH roots, canonical
unpadded AICH Base32 roots, and local AICH recovery metadata verification.
`md4` is now a workspace dependency; ED2K protocol rules remain implemented in
raria-owned code.

Verified: The RED check failed before implementation because no hash constants,
digest helpers, root hash helpers, AICH helpers, or recovery metadata model
existed. After implementation, `cargo test -p raria-ed2k hash --locked`,
`cargo fmt --all --check`, `cargo clippy -p raria-ed2k --all-targets -- -D
warnings`, and `cargo check --workspace --locked` passed.

Remaining: Start ED2K-006 bootstrap files.

Blocked: none.

## 2026-05-28 ED2K-006 verified

Changed: Added native bootstrap metadata parsing and persistence. `raria-ed2k`
now parses useful `server.met` server endpoint metadata, preserves dynip hosts,
and merges bootstrap rows without erasing existing non-empty fields. It also
parses useful `nodes.dat` Kad contacts from count-first, versioned, and
bootstrap-edition files while filtering unusable endpoints, Kad1 contacts, and
old UDP port 53 contacts. `raria-core` now has versioned native redb rows for
ED2K server and Kad bootstrap state.

Verified: The RED checks failed before implementation because no server.met
parser, nodes.dat parser, or bootstrap redb store methods existed. After
implementation, `cargo test -p raria-ed2k server_met --locked`, `cargo test -p
raria-ed2k nodes_dat --locked`, `cargo test -p raria-ed2k --locked`, `cargo
test -p raria-core ed2k_bootstrap_rows_roundtrip_by_profile --locked`, `cargo
clippy -p raria-ed2k --locked --all-targets -- -D warnings`, `cargo clippy -p
raria-core --locked --all-targets -- -D warnings`, `cargo fmt --all --check`,
and `cargo check --workspace --locked` passed.

Remaining: Start ED2K-007 stable identity and config.

Blocked: none.

## 2026-05-28 ED2K-007 verified

Changed: Added stable native ED2K identity loading and runtime configuration
projection. `raria-ed2k` can now load or create a persistent client hash from
native redb identity rows. `raria-core` now carries ED2K runtime policy from
strict `raria.toml` into `GlobalConfig`. `/api/v1/config` now exposes a native
`ed2k` object without legacy names or secrets.

Verified: The RED checks failed before implementation because the identity
loader, `GlobalConfig` ED2K fields, and API projection did not exist. After
implementation, `cargo test -p raria-ed2k identity --locked`, `cargo test -p
raria-ed2k --locked`, `cargo test -p raria-core --test native_config
--locked`, `cargo test -p raria-rpc --test native_api
config_endpoint_returns_native_runtime_projection --locked`, `cargo clippy -p
raria-ed2k --locked --all-targets -- -D warnings`, `cargo clippy -p
raria-core --locked --all-targets -- -D warnings`, and `cargo clippy -p
raria-rpc --locked --all-targets -- -D warnings` passed.

Remaining: Start ED2K-008 packet codec.

Blocked: none.

## 2026-05-28 ED2K-008 verified

Changed: Added native packet, tag, and opcode codecs in `raria-ed2k`. The
packet module now owns ED2K TCP framing, ED2K-family UDP datagrams, protocol
markers, deterministic payload limits, typed malformed-input errors, and zlib
payload wrappers through `flate2`. The tag module now round-trips compact IDs,
text names, string and short-string values, UInt8/16/32/64, bool, HASH16, BSOB,
and blob payloads. The opcode module names retained server, peer, and Kad
opcodes while leaving pruned legacy chat behavior unadvertised.

Verified: The RED check failed before implementation because public `packet`,
`tag`, and `opcode` modules did not exist. After implementation, `cargo test -p
raria-ed2k --locked`, `cargo fmt --all --check`, `cargo clippy -p raria-ed2k
--locked --all-targets -- -D warnings`, and `cargo check --workspace --locked`
passed.

Remaining: Start ED2K-009 server TCP.

Blocked: none.

## 2026-05-28 ED2K-009 verified

Changed: Added native server TCP payload and state handling in `raria-ed2k`.
The server module now builds login frames from the native client identity,
captures server TCP capabilities from IDChange, derives HighID or LowID state,
parses server status and identity tags, records server messages, builds small
and large file GetSources requests, parses normal and obfuscation-aware
FoundSources replies, and exposes a bounded retry policy. This checkpoint owns
the local parser/state layer; socket-loop scheduling remains for later runtime
integration.

Verified: The RED check failed before implementation because the server TCP
state, login builder, source request builder, FoundSources parser, LowID helper,
and retry policy did not exist. After implementation, `cargo test -p
raria-ed2k --test server_tcp --locked`, `cargo test -p raria-ed2k --locked`,
`cargo fmt --all --check`, `cargo clippy -p raria-ed2k --locked --all-targets
-- -D warnings`, `git diff --check`, and `cargo check --workspace --locked`
passed.

Remaining: Start ED2K-010 server UDP.

Blocked: none.

## 2026-05-28 ED2K-010 verified

Changed: Added native server UDP status and source-discovery payload handling
in `raria-ed2k`. The server module now builds UDP status requests, validates
challenge-bound status replies, records users, files, max users, soft/hard file
limits, UDP flags, LowID users, UDP key, and TCP/UDP obfuscation ports. It also
builds hash-only and extended hash-size UDP source requests, parses packed
FoundSources payloads while filtering unrelated hashes, stops safely at bogus
tails, and exposes bounded UDP status/source cadence policy.

Verified: The RED check failed before implementation because UDP status state,
UDP source request builders, packed source parsing, and bounded cadence helpers
did not exist. After implementation, `cargo test -p raria-ed2k --test
server_udp --locked`, `cargo test -p raria-ed2k --locked`, `cargo fmt --all
--check`, `cargo clippy -p raria-ed2k --locked --all-targets -- -D warnings`,
and `cargo check --workspace --locked` passed.

Remaining: Start ED2K-011 HighID LowID callback.

Blocked: none.

## 2026-05-28 ED2K-011 verified

Changed: Added native HighID, LowID, and server-mediated callback state in
`raria-ed2k`. The peer module now keeps HighID peers directly schedulable,
blocks LowID direct scheduling until a server callback is accepted, constructs
server callback request frames, parses callback endpoint payloads, and records
requested, accepted, failed, timed-out, impossible, and completed states.
Direct UDP callback, Kad buddy callback, and required-crypt callback remain
unadvertised until native owners exist.

Verified: The RED check failed before implementation because peer reachability,
LowID callback states, callback request construction, endpoint parsing, and
capability-limit helpers did not exist. After implementation, `cargo test -p
raria-ed2k --test callback --locked`, `cargo test -p raria-ed2k --locked`,
`cargo fmt --all --check`, `cargo clippy -p raria-ed2k --locked --all-targets
-- -D warnings`, `cargo check --workspace --locked`, and `git diff --check`
passed.

Remaining: Start ED2K-012 peer handshake.

Blocked: none.

## 2026-05-28 ED2K-012 verified

Changed: Added native ED2K peer handshake payload ownership in `raria-ed2k`.
The peer module now builds and parses hello, hello answer, eMule info, and
eMule info answer frames. Local capability truth advertises retained AICH,
Unicode, compression, Source Exchange, extended request, and large-file
metadata while keeping crypt, secure-ident, Kad peer capability, multipacket,
extended multipacket, direct callback, captcha, comments, and preview disabled
until native owners exist. Malformed handshake inputs return typed errors
without producing partial peer state.

Verified: The RED check failed before implementation because the peer
handshake API, identity model, capability model, eMule info opcode wrapper, and
typed error model did not exist. After implementation, `cargo test -p
raria-ed2k --test peer_handshake --locked`, `cargo test -p raria-ed2k
--locked`, `cargo fmt --all --check`, and `cargo clippy -p raria-ed2k --locked
--all-targets -- -D warnings` passed.

Remaining: Start ED2K-013 peer file status, hashset, queue, and request state.

Blocked: none.

## 2026-05-28 ED2K-013 verified

Changed: Added native ED2K peer request-state ownership in `raria-ed2k`.
The peer module now builds plain fallback RequestFileName,
SetRequestedFileId, HashsetRequest, StartUploadRequest, FileStatus, and
HashsetAnswer frames. It parses file-status bitfields, hashset answers, and
two-byte or four-byte queue-rank payloads with typed errors. `PeerRequestState`
now records part status, piece hashes, queue rank, peer-owned requested
ranges, upload acceptance, and explicit no-needed-parts, no-file, out-of-parts,
cancelled, and failed phases.

Verified: The RED check failed before implementation because request payload
builders, parsers, request phases, and failure cleanup state did not exist.
After implementation, `cargo test -p raria-ed2k --test peer_request_state
--locked`, `cargo test -p raria-ed2k --locked`, `cargo fmt --all --check`,
and `cargo clippy -p raria-ed2k --locked --all-targets -- -D warnings`
passed.

Remaining: Start ED2K-014 Source Exchange and source lifecycle.

Blocked: none.

## 2026-05-28 ED2K-014 verified

Changed: Added native Source Exchange and source lifecycle ownership in
`raria-ed2k`. The new `source` module builds SX1 and SX2 source requests,
builds and parses versioned Source Exchange answers, preserves endpoint,
server, user-hash, and crypt option metadata, and returns typed errors for bad
payloads. It also owns source merge and scheduling policy for useful endpoints,
duplicates, self and loopback rejection, required-crypt exclusion, origin
updates, active caps, queue retry, no-needed-parts quality, and dead-source
expiry.

Verified: The RED check failed before implementation because no source module,
Source Exchange payload helpers, source model, or lifecycle policy existed.
After implementation, `cargo test -p raria-ed2k --test source_exchange
--locked`, `cargo test -p raria-ed2k --locked`, `cargo fmt --all --check`, and
`cargo clippy -p raria-ed2k --locked --all-targets -- -D warnings` passed.

Remaining: Start ED2K-015 part request planning and I64 offsets.

Blocked: none.

## 2026-05-28 ED2K-015 verified

Changed: Added native ED2K part request planning in `raria-ed2k`. The
transfer module now plans non-overlapping AICH emblock ranges while respecting
completed ranges, globally owned ranges, peer-owned outstanding ranges, remote
part availability, known file size, and last partial blocks. It also builds and
parses retained RequestParts and RequestPartsI64 payloads with typed errors for
invalid ranges, too many ranges, legacy offset overflow, truncated payloads,
and file-hash mismatch.

Verified: The RED check failed before implementation because part planning,
RequestParts codec helpers, I64 offsets, and typed payload errors did not
exist. A later focused run exposed two defects: existing peer-owned ranges were
incorrectly counted against the new request frame capacity, and short payloads
with a readable wrong hash returned `Truncated` before `HashMismatch`. After
fixing those root causes, `cargo test -p raria-ed2k --test part_planning
--locked`, `cargo test -p raria-ed2k --locked`, `cargo fmt --all --check`,
`cargo clippy -p raria-ed2k --locked --all-targets -- -D warnings`, and
`cargo check --workspace --locked` passed.

Remaining: Start ED2K-016 compressed parts, cancellation, timeout, and retry.

Blocked: none.

## 2026-05-28 ED2K-016 verified

Changed: Added native ED2K part payload validation and transfer failure
handling in `raria-ed2k`. The transfer module now decodes normal and I64 part
payloads only after validating file hash, declared range, payload length, file
size, and peer-owned range. It also owns streaming compressed-part inflation
with `flate2`, compressed length checks, output bounds, cancel-frame
construction, timeout expiry, requested-range cleanup, and bounded retry
classification.

Verified: The RED check failed before implementation because part payload
decoding, compressed chunk inflation, cancel-frame construction, and transfer
failure state did not exist. After implementation, `cargo test -p raria-ed2k
--test part_transfer --locked`, `cargo test -p raria-ed2k --locked`, `cargo
fmt --all --check`, focused raria-ed2k clippy with `-D warnings`, and `cargo
check --workspace --locked` passed.

Remaining: Start ED2K-017 integrity, disk resume, and completion truth.

Blocked: none.

## 2026-05-28 ED2K-017 verified

Changed: Added native ED2K verified-byte and resume ownership. `raria-ed2k`
now has `Ed2kDiskState` for staged durable writes, incomplete-part rejection,
MD4-gated verified ranges, corrupt-range requeue, AICH root retention, source
resume snapshots, and versioned resume snapshots. `raria-core` now has
`NativeEd2kResumeRow`, `NativeEd2kResumeSourceRow`, and an `ed2k_resume` redb
table keyed by native task id.

Verified: The RED checks failed before implementation because disk truth,
corrupt requeue, resume snapshots, and native ED2K resume rows did not exist.
After implementation, `cargo test -p raria-ed2k --test disk_resume --locked`,
`cargo test -p raria-core ed2k_resume_rows_roundtrip_by_task --locked`, `cargo
test -p raria-ed2k --locked`, `cargo fmt --all --check`, focused raria-core
and raria-ed2k clippy with `-D warnings`, and `cargo check --workspace
--locked` passed.

Remaining: Start ED2K-018 Kad bootstrap and routing table.

Blocked: none.

## 2026-05-28 ED2K-018 verified

Changed: Added native Kad routing ownership. `raria-ed2k` now represents Kad
contacts with optional UDP keys, validates routing contacts, maintains a
128-bucket routing table with bounded live and replacement contacts, promotes
confirmed replacements, handles failure replacement, sorts closest contacts by
XOR distance, excludes requesters, gates bootstrap and refresh cadence, stores
serializable routing snapshots, and tracks Kad UDP transactions through
completion and expiry. `raria-core` now has `NativeEd2kKadRoutingRow` and the
`ed2k_kad_routing` redb table for native profile-scoped routing snapshots.

Verified: The RED check failed before implementation because Kad routing table,
contact validation, transaction table, and UDP-key representation did not
exist. After implementation, `cargo test -p raria-ed2k --test kad_routing
--locked`, `cargo test -p raria-core
ed2k_kad_routing_rows_roundtrip_by_profile --locked`, `cargo test -p
raria-ed2k --locked`, `cargo fmt --all --check`, focused raria-core and
raria-ed2k clippy with `-D warnings`, and `cargo check --workspace --locked`
passed.

Remaining: Start ED2K-019 Kad source search, publish, and keyword search.

Blocked: none.

## 2026-05-28 ED2K-019 verified

Changed: Added native Kad source search, source publish, and keyword search
ownership in `raria-ed2k`. The Kad module now owns bounded traversal actions,
source-search request payloads, search result payload parsing, direct source
tag extraction, source-policy merge through `SourceLifecycle`, sharing-gated
source publish payloads, large-file source type truth, keyword target hashing,
keyword request payloads, and result-id dedupe.

Verified: The RED check failed before implementation because Kad traversal,
source search payloads, publish payloads, keyword target hashing, and search
entry dedupe did not exist. After implementation, `cargo test -p raria-ed2k
--test kad_search --locked`, `cargo test -p raria-ed2k --locked`, `cargo fmt
--all --check`, focused raria-ed2k clippy with `-D warnings`, and `cargo check
--workspace --locked` passed.

Remaining: Start ED2K-020 Kad firewall state, buddy limits, and scheduling.

Blocked: none.

## 2026-05-28 ED2K-020 verified

Changed: Added native Kad firewall state and explicit listen policy. `raria-ed2k`
now has `KadFirewallState` for TCP and UDP reachability status, manual
firewalled assumptions, bounded check cadence, UDP reachability, and direct
source-publish gating. `raria.toml` `[ed2k]` now carries
`assume_firewalled`, and `/api/v1/config` exposes it through the native config
projection. Router helper options remain rejected.

Verified: The RED checks failed before implementation because Kad firewall
state and `assume_firewalled` did not exist. After implementation, `cargo test
-p raria-ed2k --test kad_firewall --locked`, `cargo test -p raria-core --test
native_config --locked`, `cargo test -p raria-rpc --test native_api
config_endpoint_returns_native_runtime_projection --locked`, `cargo test -p
raria-ed2k --locked`, `cargo fmt --all --check`, focused clippy for touched
crates with `-D warnings`, and `cargo check --workspace --locked` passed.

Remaining: Start ED2K-021 shared file store and publishing.

Blocked: none.

## 2026-05-28 ED2K-021 verified

Changed: Added native ED2K shared-file store ownership. `raria-ed2k` now has
`SharedFileStore` for completed and explicitly imported files, strict metadata
validation through ED2K disk truth, duplicate root-hash replacement,
sharing-gated server and Kad publish records, and verified-range disk reads for
future upload responders. Legacy `known.met`, preview, browsing UI, and old
database import behavior remain absent.

Verified: The RED check failed before implementation because shared metadata,
publish records, and verified shared reads did not exist. After implementation,
`cargo test -p raria-ed2k --test sharing_store --locked`, `cargo test -p
raria-ed2k --locked`, `cargo fmt --all --check`, focused raria-ed2k clippy
with `-D warnings`, and `cargo check --workspace --locked` passed.

Remaining: Start ED2K-022 upload queue, UDP reask, and responder.

Blocked: none.

## 2026-05-28 ED2K-022 verified

Changed: Added native ED2K upload queue and responder ownership. `raria-ed2k`
now has `UploadQueue` for active slot caps, waiting ranks, duplicate user-hash
rejection, deterministic waiting order, and promotion after active-peer removal.
Shared upload decisions now build native TCP response frames for accepted,
queued, missing-file, duplicate, and full-queue cases. Shared part serving reads
verified shared ranges and emits normal or I64 part payloads. UDP reask handling
returns rank-zero ACKs only for active uploads, ranked ACKs for waiting peers,
FileNotFound for missing or mismatched files, and QueueFull for unknown peers.

Verified: The RED check failed before implementation because upload queue
state, upload response frames, shared-part frames, and UDP reask responses did
not exist. After implementation, `cargo test -p raria-ed2k --test
upload_queue --locked`, `cargo test -p raria-ed2k --locked`, `cargo fmt --all
--check`, focused raria-ed2k clippy with `-D warnings`, and `cargo check
--workspace --locked` passed.

Remaining: Start ED2K-023 credits and secure-ident truth.

Blocked: none.

## 2026-05-28 ED2K-023 verified

Changed: Added native ED2K credit ownership. `raria-ed2k` now has
`PeerCreditStore` for uploaded and downloaded byte counters, native snapshot
roundtrip, and bounded eMule-style score ratios. `UploadQueue` owns the credit
store and uses score ratios only for waiting-peer ordering, preserving active
slot priority and deterministic endpoint tie breaks. `raria-core` now stores
versioned `NativeEd2kCreditRow` records in the `ed2k_credits` redb table.
Secure identification remains unadvertised because no public-key and signature
flow exists.

Verified: The RED checks failed before implementation because credit counters,
queue score ordering, and native credit persistence did not exist. After
implementation, `cargo test -p raria-ed2k --test credits --locked`, `cargo
test -p raria-core ed2k_credit_rows_roundtrip_by_profile --locked`, `cargo
test -p raria-ed2k --locked`, `cargo fmt --all --check`, focused raria-core
and raria-ed2k clippy with `-D warnings`, and `cargo check --workspace
--locked` passed.

Remaining: Start ED2K-024 native API, events, CLI, and daemon integration.

Blocked: none.

## 2026-05-28 ED2K-024 verified

Changed: Wired ED2K into the raria-native integration surface. `/api/v1/tasks`
now creates native ED2K jobs from ED2K links and task summaries expose compact
ED2K runtime status fields. `/api/v1/events` streams stable `task.ed2k.*`
events with `ed2kStatus` payloads for source, peer, queue, Kad, transfer,
sharing, upload, and search updates. The daemon no longer fails ED2K tasks at
activation; it publishes native ED2K status and waits for cancellation until the
protocol runtime is attached. `raria download` accepts ED2K links as native URL
inputs. Native task persistence now stores and restores the backend kind so ED2K
session rows do not come back as range downloads.

Verified: The RED check failed before implementation because native task rows
did not preserve ED2K backend kind. After implementation, focused ED2K checks,
`cargo test -p raria-core --locked`, `cargo test -p raria-rpc --test
native_api --locked`, `cargo test -p raria-cli ed2k --locked`, `cargo test
--workspace --locked`, `cargo fmt --all --check`, `cargo check --workspace
--locked`, and focused clippy for touched crates with `-D warnings` passed. A
pre-existing native API smoke port race was fixed after the first workspace run
reported a local address-in-use failure.

Remaining: Start ED2K-025 search API, status docs, and stale-surface scans.

Blocked: none.

## 2026-05-28 ED2K-025 verified

Changed: Added native ED2K search resources. `/api/v1/ed2k/searches` now
creates and lists opaque search resources with native server/Kad network
selection. `/api/v1/ed2k/searches/{searchId}` returns paged results with
startable `ed2kUri` links. README and the ED2K overview now describe the
implemented ED2K surface accurately: task creation, status projection, events,
search resources, protocol primitives, and persistence are present, while live
ED2K network transfer runtime remains open until the runtime checkpoints close.

Verified: The RED checks failed before implementation because the native search
routes, search ids, result model, and result-recording hook did not exist. After
implementation, `cargo test -p raria-rpc --test native_api ed2k_search
--locked` passed. CSV validation and stale-surface scans found no legacy public
search method, JSON-RPC revival, or false full-runtime claim.

Remaining: Start ED2K-026 runtime checkpoint reset.

Blocked: none.

## 2026-05-28 ED2K-026 verified

Changed: Reset the ED2K tracker tail so the workstream no longer treats native
API resources and protocol primitives as final runtime completion. The daemon
still publishes an ED2K waiting-for-runtime status and waits for cancellation,
so the remaining work is now split into runtime orchestration, server runtime,
Kad runtime, peer download runtime, disk and sharing runtime, and final
validation checkpoints.

Verified: Source inspection confirmed the daemon placeholder. CSV validation
covered the tracker files. Stale-surface scans found no JSON-RPC, aria2 method,
legacy Motrix adapter, or false full-runtime claim introduced by the reset.

Remaining: Start ED2K-027 runtime orchestration.

Blocked: none.

## 2026-05-28 ED2K-027 verified

Changed: Added the native ED2K runtime context and scheduler boundary.
`raria-ed2k` now owns projected runtime config, task-scoped context, startup
statuses, and bounded scheduler tick status. The daemon now routes ED2K tasks
through that context and publishes native `task.ed2k.*` source, queue, Kad,
sharing, and transfer updates until cancellation.

Verified: RED checks failed before `raria_ed2k::runtime` existed and before the
daemon stopped emitting the placeholder waiting state. After implementation,
`cargo test -p raria-ed2k --test runtime --locked`, `cargo test -p raria-cli
ed2k_runtime_waits_for_cancellation_without_failing_task --locked`, `cargo test
-p raria-ed2k --locked`, and `cargo test -p raria-cli ed2k --locked` passed.

Remaining: Start ED2K-028 server TCP UDP runtime source discovery.

Blocked: none.

## 2026-05-28 ED2K-028 verified

Changed: Added live ED2K server TCP and UDP exchange ownership inside
`raria-ed2k`. `Ed2kServerRuntime` can contact a server endpoint over TCP,
send native login and source requests, apply ID and status responses, and
return matching sources. It can also send UDP status and source requests,
accept challenge-bound status, and return matching UDP source records. The
runtime context now has a source-recording hook for server-discovered source
counts.

Verified: RED checks failed before the server runtime types existed. After
implementation, local socket tests covered TCP and UDP exchanges without public
network access. `cargo test -p raria-ed2k --test server_runtime --locked`,
`cargo test -p raria-ed2k --test runtime --locked`, and `cargo test -p
raria-ed2k --locked` passed.

Remaining: Start ED2K-029 Kad UDP runtime source discovery and publish.

Blocked: none.
