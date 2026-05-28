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
