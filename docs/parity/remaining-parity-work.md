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

- **Protocol matrix:** `has_code` rows are now cleared; remaining protocol work is concentrated in `wired` promotions plus explicit accepted gaps
- **RPC matrix:** `has_code` rows are now cleared; the remaining open RPC work is primarily `wired` → `tested/client_verified`
- **Accepted gaps:** BT-GAP-001 through BT-GAP-004 remain intentional gaps today
- **Next promotion candidates:** the remaining `wired` rows with fresh daemon/binary evidence

## Replacement-Core Hard Gate

These items still sit on the replacement path described in the consensus plan, so they should close
before claiming aria2-grade daemon replacement.

| Ledger row | Current | Evidence already in repo | What still blocks closure |
| --- | --- | --- | --- |
| HTTP resume (partial download) | `tested` | `crates/raria-core/tests/segment_checkpoint.rs`, `crates/raria-cli/tests/session_smoke.rs`, `crates/raria-cli/tests/single_download.rs` | Remaining work is about broader `client_verified` confidence, not missing hot-path implementation |
| `aria2.changeGlobalOption` | `client_verified` | `crates/raria-cli/tests/rpc_smoke.rs`, `crates/raria-rpc/tests/options_parity.rs` | No blocker on core replacement path; ledger retained here as a closed core contract |

## Secondary Protocol Promotion

These rows are valuable parity work, but the plan treats them as follow-up promotion lanes rather
than immediate replacement blockers.

| Theme | Ledger rows | Current status | Existing evidence | Next gate |
| --- | --- | --- | --- | --- |
| FTP lifecycle hardening | Explicit FTPS daemon-path confidence | `client_verified` | Backend and single-download binary path now both verify explicit FTPS with AUTH TLS + PBSZ/PROT + protected data transfer under a custom trusted CA | Next gate is daemon-path confidence before calling the entire FTPS lane fully closed |
| SFTP end-to-end coverage | daemon-path confidence only | `client_verified` | Dedicated in-process SFTP smoke plus binary-path smoke now cover password auth, key auth, strict known_hosts, and SOCKS5 proxy transport | Next gate is daemon-path confidence if we want to push the SFTP lane beyond binary-path replacement readiness |
| BitTorrent runtime fidelity | DHT, PEX, fastresume, SOCKS5 proxy, peer/detail UX | `wired` | librqbit-backed service, BT dispatch tests, and daemon RPC smoke now cover pause/resume on the real daemon path; BT session init also now consumes SOCKS5 proxy config with automated validation | Add product-path verification for peers/proxy/transport-specific lanes |
| Metalink runtime verification | Chunk checksum, Metalink/HTTP | `gap` | Parser and normalizer cover file-level hashes and URL ordering only | Accepted gaps unless the project later chooses to add piece-level Metalink models and RFC 6249 header discovery |

## Compatibility Tail

These items are still real compatibility work, but the consensus plan treats them as polish or
lower-value follow-ups instead of core replacement gates.

| Theme | Ledger rows | Current status | Review note |
| --- | --- | --- | --- |
| Proxy and TLS polish | Explicit FTPS daemon path | `client_verified` | SOCKS5-backed FTP/SFTP transport hooks now have dedicated protocol-specific smoke coverage, and explicit FTPS is verified on both backend and single-download binary paths; the remaining gap is daemon-path parity confidence |
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
