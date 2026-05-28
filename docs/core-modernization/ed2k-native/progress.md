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
