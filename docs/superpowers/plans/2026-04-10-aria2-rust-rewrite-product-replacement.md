# Raria Product-Replacement Rewrite Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `raria` as a Rust-native, product-grade replacement for aria2, prioritizing real client/workflow compatibility over strict recreation of every legacy detail.

**Architecture:** Keep the current layered Rust design (`raria-core` / `raria-range` / protocol backends / `raria-rpc` / `raria-cli` / `raria-bt`) and finish the missing product loops around daemon operation, session/resume semantics, RPC/client behavior, and BitTorrent file-level usability. Preserve compatibility where it is cheap or naturally supported by mature crates; explicitly downgrade or defer legacy corners that would force bespoke subsystem work.

**Tech Stack:** `tokio`, `reqwest`, `jsonrpsee`, `redb`, `governor`, `quick-xml`, `suppaftp`, `russh` + `russh-sftp`, `librqbit`, `tracing` + `tracing-subscriber`, plus selective additions such as `tracing-appender`, `daemonize`, and `arc-swap`.

---

## Objective Verdict On The Candidate Plan

The Claude plan is useful, but it should not be adopted verbatim.

What is strong:

- It aligns with the repository's own parity ledgers in [`docs/parity/option-tiers.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/option-tiers.md), [`docs/parity/protocol-matrix.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/protocol-matrix.md), and [`docs/parity/rpc-matrix.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/rpc-matrix.md).
- It correctly keeps mature Rust libraries as the default implementation strategy.
- It identifies several real product gaps: daemonization, session interval saving, runtime global-limit mutation, BT notifications, and better end-to-end verification.

What is wrong or too optimistic:

- It still behaves like a parity-backlog plan more than a product-release plan.
- It treats too many `has_code` items as equally release-blocking.
- It assumes some BT behaviors are only wiring work when they may be semantic mismatches with `librqbit`.
- It promotes some ops features to GA while underweighting session semantics and real client workflows.
- It keeps hook scripts too high for a product-replacement-first release.
- It does not clearly separate:
  - GA hard gate
  - compatibility tail
  - accepted permanent gaps

This plan fixes those problems.

## Source Of Truth

Use these as the planning baseline:

- Deep-interview spec: [`/.omx/specs/deep-interview-aria2-rust-rewrite.md`](/Users/sekiro/Projects/VSCode/raria/.omx/specs/deep-interview-aria2-rust-rewrite.md)
- Option parity ledger: [`docs/parity/option-tiers.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/option-tiers.md)
- Protocol parity ledger: [`docs/parity/protocol-matrix.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/protocol-matrix.md)
- RPC parity ledger: [`docs/parity/rpc-matrix.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/rpc-matrix.md)
- BT known differences: [`docs/parity/bt-gap-ledger.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/bt-gap-ledger.md)

## Product Strategy

### North Star

`raria` becomes the maintained Rust implementation that users can run instead of aria2 for real-world daemon, RPC, CLI, HTTP/FTP/SFTP, Metalink, and mainstream BitTorrent workflows.

### GA Hard Gate

The first "rewrite complete" release is the first release where:

- AriaNg can connect to `raria` and complete normal workflows without visible breakage
- Motrix-like RPC clients can use `raria` as a daemon backend without major feature loss
- Single-shot CLI downloads behave reliably for mainstream use
- session persistence and restart/resume work in product flows
- HTTP/FTP/SFTP/Metalink and mainstream BT downloads are good enough to replace aria2 for the majority of users

### Compatibility Tail

Important but non-blocking compatibility work that improves parity after GA.

### Accepted Gaps

Legacy features that should not block GA because they would force bespoke subsystem work or fight the chosen ecosystem crates.

## Non-Goals For GA

Do not block GA on:

- strict `.aria2` control-file compatibility
- XML-RPC
- implicit FTPS
- ecosystem-hostile BT details unsupported by `librqbit`
- perfect recreation of every legacy CLI option behavior
- exotic proxy combinations with little evidence of real client dependence

## Architecture Direction

### Keep

- `raria-core` as the lifecycle / scheduler / persistence / cancel backbone
- `raria-range` as the segmented executor for byte-range protocols
- `raria-http`, `raria-ftp`, `raria-sftp` as protocol-specific backends
- `raria-rpc` as the aria2-compatible external surface
- `raria-bt` as the BT facade around `librqbit`

### Do Not Recreate

- the legacy command hierarchy in `aria2-legacy`
- legacy event-poll abstraction layers
- custom BT stack, DHT parser, or RPC engines if modern crates already solve them

### Core Principle

When a mature crate already owns a protocol well, `raria` should own:

- product semantics
- integration
- lifecycle
- persistence
- compatibility mapping

It should not own the whole protocol engine unless absolutely necessary.

## Ecosystem Decisions

Keep and trust:

- `librqbit` for torrent session handling and mainstream BT functionality. Docs indicate it provides `Session`, `AddTorrentOptions`, `ManagedTorrent`, and DHT support, which is enough for a product BT base without reimplementing the torrent stack. Source: [librqbit docs](https://docs.rs/librqbit/latest/librqbit/)
- `jsonrpsee` for async JSON-RPC over HTTP and WebSocket. Its docs describe it as the successor to Parity JSONRPC and expose server-side method, connection, and subscription primitives. Source: [jsonrpsee docs](https://docs.rs/crate/jsonrpsee/latest)
- `suppaftp` for FTP/FTPS. Its tokio module exposes async FTP streams and FTPS data-stream types; this should be integration work, not protocol reimplementation. Source: [suppaftp tokio docs](https://docs.rs/suppaftp/latest/suppaftp/tokio/index.html)
- `russh-sftp` for SFTP. Its docs explicitly provide client and server support, including a high-level async-I/O client API. Source: [russh-sftp docs](https://docs.rs/russh-sftp/latest/russh_sftp/)
- `tracing-appender` for non-blocking file logging. Source: [tracing-appender docs](https://docs.rs/tracing-appender/latest/tracing_appender/)
- `arc-swap` for hot-path read-mostly config mutation, especially runtime limit changes. Source: [arc-swap docs](https://docs.rs/arc-swap/latest/arc_swap/)

Adopt selectively:

- `daemonize` only for Unix daemon mode. Source: [daemonize docs](https://docs.rs/daemonize/latest/daemonize/)
- `digest_auth` only if HTTP Digest is kept in the compatibility tail. Source: [digest_auth docs](https://docs.rs/digest_auth/latest/digest_auth/)

## Improved Release Plan

### Release 1: Product Replacement Beta

This is the first release that should be considered the real end of the "rewrite not yet finished" era.

Hard blockers:

- daemon behavior is complete enough for long-running use
- session save/restore and restart/resume are reliable
- AriaNg core workflows are verified
- RPC global-option mutation behaves correctly for the options clients actually use
- BT file selection works
- product-visible status fields are good enough for clients

Not hard blockers:

- hook scripts
- Digest auth
- strict legacy file compatibility
- BT peer-detail polish

### Release 2: Compatibility Expansion

After Beta, add high-value parity improvements that increase migration confidence without destabilizing the architecture.

### Release 3: Long-Tail Compatibility / Accepted-Gap Review

Review whether any accepted gap has become cheap because upstream crates now support it.

## Workstream Plan

### Workstream A: Daemon And Product Operations

Files:

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/main.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/config.rs`

Goals:

- implement Unix daemon mode
- support file logging
- support periodic session persistence
- improve signal handling

Recommended crate additions:

- `daemonize`
- `tracing-appender`

Assessment:

- `--daemon` is GA-critical
- file logging is important for operability but not itself a replacement blocker
- `SIGUSR1` parity is useful, but product restart safety matters more than signal fidelity

### Workstream B: Session And Resume Semantics

Files:

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/engine.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/persist.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/single.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-range/src/executor.rs`

Goals:

- make restart/resume fully trustworthy in the product path
- confirm segment checkpoint semantics are sufficient
- prefer `redb` state as the canonical source of truth
- do not introduce `.aria2` file compatibility as a GA requirement

Assessment:

- This is more important than several compatibility-tail items in the Claude draft
- If restart/resume is unreliable, `raria` is not a real aria2 replacement no matter how many CLI flags parse

### Workstream C: RPC And Client Workflow Completeness

Files:

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/facade.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/engine.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/job.rs`

Goals:

- add browser-safe HTTP CORS handling for browser-hosted AriaNg-style deployments
- make `changeGlobalOption` affect live runtime behavior where expected
- expose non-fake `connections`
- close the gap between "tested RPC method exists" and "real client UX is correct"

Recommended crate additions:

- `tower-http`
- `arc-swap`

Assessment:

- For direct browser-to-daemon AriaNg deployments, CORS is a practical GA blocker because the current server path in [`server.rs`](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs) builds a plain `jsonrpsee::server::Server` with no HTTP middleware or origin handling.
- This should **not** default to `CorsLayer::permissive()` in production. The better product shape is:
  - configurable "allow all" behavior equivalent to aria2's `rpc-allow-origin-all`
  - optional explicit allowlist/origin setting for safer browser access
  - no unnecessary CORS relaxation when the deployment is same-origin or reverse-proxied
- This is GA-critical
- It should be verified through AriaNg, not only unit tests

### Workstream D: BitTorrent Product Usability

Files:

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/bt_runtime.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/facade.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/events.rs`

Goals:

- BT file selection
- better `getFiles` behavior for torrents
- pause/resume confidence in product flows
- optional BT-specific completion notification

Assessment:

- BT file selection is GA-critical because it is visible in real clients
- `onBtDownloadComplete` is not GA-critical
- DHT/PEX/uTP remain "use upstream behavior, do not reimplement"

### Workstream E: Protocol Hardening

Files:

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-http/src/backend.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-ftp/src/backend.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-sftp/src/backend.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/file_alloc.rs`

Goals:

- fix FTP lifecycle cleanup
- verify SFTP key auth end-to-end rather than treating it as missing greenfield work
- improve file allocation semantics where practical
- leave Digest auth as compatibility-tail work unless product evidence shows it is needed now

Assessment:

- Claude's plan overstated the urgency of Digest auth
- It understated the importance of turning currently `wired` SFTP/FTP paths into actual client-verified behavior

### Workstream F: Metalink Realism

Files:

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-metalink/src/parser.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-metalink/src/normalizer.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs`

Goals:

- ensure parsed hashes are actually enforced in product flows
- add practical multi-mirror failover
- defer RFC 6249 Metalink/HTTP unless a real user workflow demands it

Assessment:

- Hash-chaining and failover are worth doing
- Metalink/HTTP is not a GA blocker

### Workstream G: Compatibility Tail

Files:

- Modify as needed across CLI / protocol / RPC crates

Includes:

- hook scripts
- Digest auth
- save-cookies writeback
- BT peer-detail polish
- BT `seed-ratio` / `seed-time`
- extra tracker injection
- mTLS client certs
- selective proxy-path improvements

Assessment:

- Hook scripts should move here instead of GA
- They are valuable, but they do not block AriaNg/Motrix replacement

## Accepted Gaps

Keep as accepted gaps unless upstream changes:

- XML-RPC
- implicit FTPS
- `.aria2` control-file format
- BT encryption (MSE/PSE)
- WebSeed
- rarest-first semantics
- mixed HTTP + BT source download
- low-value legacy proxy corners

## Verification Matrix

GA should be defined by passing these flows, not by clearing every table row:

- AriaNg:
  - connect
  - add URI
  - pause
  - resume
  - observe progress/speed/connections
  - change global speed limit
  - reconnect after daemon restart
- Motrix-like daemon flow:
  - queue task
  - daemon restart
  - session restore
- CLI:
  - direct HTTP download
  - checksum verification
  - interruption + resume
- BT:
  - add magnet/torrent
  - view files
  - select subset
  - pause/resume

## Recommended Dependency Policy

Add now:

- `tower-http`
- `daemonize`
- `tracing-appender`
- `arc-swap`

Add only if the relevant tail work is accepted:

- `digest_auth`
- `wiremock` if current test infrastructure becomes too awkward, though it is still a good dev-dependency for protocol verification

## Exit Criteria

Declare the rewrite "product-complete" when:

- GA verification matrix passes
- no Tier A option that matters to AriaNg/Motrix/real daemon use remains at `has_code`
- the major BT/HTTP/FTP/SFTP/Metalink product loops are stable
- remaining incompatibilities are either:
  - clearly documented accepted gaps, or
  - compatibility-tail backlog items that do not undermine replacement use

## Next Planning Split

After approval, split implementation into separate execution plans:

1. daemon + session + resume
2. RPC/AriaNg client behavior
3. BT file selection + torrent UX
4. protocol hardening
5. compatibility tail
