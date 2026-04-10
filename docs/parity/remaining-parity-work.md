# Remaining Parity Work Review

> Updated: 2026-04-10
>
> Scope: only the rows that are still open in [`protocol-matrix.md`](./protocol-matrix.md) and
> [`rpc-matrix.md`](./rpc-matrix.md).
>
> Lane assignments below are derived from
> [`docs/superpowers/plans/2026-04-10-aria2-rust-rewrite-consensus-plan.md`](../superpowers/plans/2026-04-10-aria2-rust-rewrite-consensus-plan.md)
> so the parity ledgers, plan, and code review notes stay aligned.

## Snapshot

- **Protocol matrix:** only a small set of protocol rows remain open (`wired`, `has_code`, or `gap`)
- **RPC matrix:** only a small set of RPC rows remain open after promoting `aria2.changeGlobalOption` to `client_verified`
- **Accepted gaps:** BT-GAP-001 through BT-GAP-004 remain intentional gaps today
- **Next promotion candidates:** the remaining `wired` rows with fresh daemon/binary evidence

## Replacement-Core Hard Gate

These items still sit on the replacement path described in the consensus plan, so they should close
before claiming aria2-grade daemon replacement.

| Ledger row | Current | Evidence already in repo | What still blocks closure |
| --- | --- | --- | --- |
| HTTP resume (partial download) | `wired` | `crates/raria-core/tests/segment_checkpoint.rs`, `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-cli/tests/single_download.rs` | Current evidence proves daemon restart resume and minimal single-download `--continue`, but not the full aria2 continuation surface |
| `aria2.changeGlobalOption` | `client_verified` | `crates/raria-cli/tests/rpc_smoke.rs`, `crates/raria-rpc/tests/options_parity.rs` | No blocker on core replacement path; ledger retained here as a closed core contract |

## Secondary Protocol Promotion

These rows are valuable parity work, but the plan treats them as follow-up promotion lanes rather
than immediate replacement blockers.

| Theme | Ledger rows | Current status | Existing evidence | Next gate |
| --- | --- | --- | --- | --- |
| FTP lifecycle hardening | FTP basic download, passive mode, range/resume, explicit FTPS | `wired` | Backend exists in `crates/raria-ftp`; explicit FTPS hot path now performs `AUTH TLS` | Add dedicated binary-path or daemon-path FTP/FTPS E2E before promotion beyond `wired` |
| SFTP end-to-end coverage | SFTP basic/password/key auth/host verification | `wired` | Backend + config live in `crates/raria-sftp`, `crates/raria-core/src/config.rs` | Add real SFTP integration coverage, especially key-auth happy path |
| BitTorrent runtime fidelity | DHT, PEX, uTP, pause/resume, fastresume, SOCKS5 proxy | `wired` | librqbit-backed service and BT dispatch tests | Add product-path BT verification, especially pause/resume and peer/detail UX |
| Metalink runtime verification | Chunk checksum, Metalink/HTTP | `gap` | Parser and normalizer cover file-level hashes and URL ordering only | Accepted gaps unless the project later chooses to add piece-level Metalink models and RFC 6249 header discovery |

## Compatibility Tail

These items are still real compatibility work, but the consensus plan treats them as polish or
lower-value follow-ups instead of core replacement gates.

| Theme | Ledger rows | Current status | Review note |
| --- | --- | --- | --- |
| Proxy and TLS polish | FTP proxy, SFTP proxy | `has_code` | Current transport crates do not yet provide a ready-made product path in this repo; treat as explicit follow-up or accepted gap, not silent backlog |
| Core daemon ergonomics | Remaining CLI/session polish rows still at `wired` | `wired` | Promote only when daemon/binary smoke covers the real product path |

## Accepted Gaps

These gaps are already documented as acceptable product differences unless upstream capabilities
change materially.

| Gap | Matrix rows | Why it stays a gap today |
| --- | --- | --- |
| BT-GAP-001 | BitTorrent MSE/PSE encryption | Blocked on upstream librqbit encryption support |
| BT-GAP-002 | BitTorrent WebSeed (BEP-17/19) | Upstream issue remains open |
| BT-GAP-003 | BitTorrent rarest-first | Current sequential strategy is an accepted behavioral difference |
| BT-GAP-004 | HTTP+BT mixed-source download | Treated as an aria2-specific feature, not a near-term parity target |
| Metalink chunk checksum | Metalink chunk checksum | Piece-level Metalink verification is not represented in the current parser/runtime model and is explicitly deferred |
| Metalink/HTTP (RFC 6249) | Metalink/HTTP | Dynamic HTTP-header-based mirror discovery is explicitly outside the current replacement scope |

See [`bt-gap-ledger.md`](./bt-gap-ledger.md) for the per-gap rationale and ignored test coverage.
