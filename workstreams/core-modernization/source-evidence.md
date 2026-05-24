# Source Evidence

This file defines how evidence is gathered for the raria core-modernization
tracker. It replaces the old source-audit notes as the active source policy.

## Active Inputs

raria workspace: `/Users/sekiro/Projects/personal/raria`

aria2-next modernization reference:
`/Users/sekiro/Projects/personal/aria2-next/docs/maintenance/core-modernization`

Included raria inputs are workspace manifests, crate manifests, `Cargo.lock`,
all workspace Rust source, all workspace tests, repository Markdown docs,
toolchain and formatting configuration, Git status, and current diff.

Excluded inputs are dependency source code, generated build output, `.git`,
`target`, local caches, temporary smoke outputs, packet captures, and editor
artifacts.

## Reference Use

aria2-next is the primary modernization reference. Use it to derive tracker
shape, deletion discipline, dependency-ledger style, checkpoint sizing, stale
scan expectations, and library-first ownership rules.

raria is not required to preserve aria2-next public surfaces. aria2-next keeps
some product choices that raria rejects, including JSON-RPC as a supported
surface, Motrix integration, ED2K, C++ packaging concerns, and libcurl/OpenSSL
ownership. raria replaces those with Rust-native public surfaces and Rust
library ownership.

## Old raria Documents

The previous modernization documents under `docs/modernization` are migration
inputs only. Their useful evidence has been consolidated into this tracker.
They are not an active authority after `CM-001` is verified.

Important migrated facts:

| Old evidence | New owner |
| --- | --- |
| Prior run reached multi-source failover | `progress.md` and `roadmap.csv` baseline notes |
| Native API and event progress | `capability-ledger.csv` |
| Remaining `Gid`, `Job`, JSON-RPC, parity, and compatibility debt | `capability-ledger.csv` and `CM-003` through `CM-020` |
| Selected Rust libraries | `dependency-ledger.csv` |
| Native architecture target | `overview.md` and capability ledger rows |
| Validation ladder | `overview.md` |

## Internet Use

Use internet search only when current library APIs, protocol details, latest
versions, or upstream limitations cannot be proven from local source and lock
files. Prefer official docs, docs.rs, RFCs, and upstream repositories. Record
only concise decisions in `dependency-ledger.csv`; do not turn project docs
into external link collections.

## Evidence Standard

Every implementation checkpoint must identify the local source, test, or docs
that prove the current state. If a feature is deleted, record the deleted
surface and the native behavior or explicit exclusion that makes deletion
valid. If a feature remains unimplemented, record whether it is excluded or a
proven technical limitation.
