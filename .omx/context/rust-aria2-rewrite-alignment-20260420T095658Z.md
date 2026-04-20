Task statement

- Align on the product target for `raria`: a modern Rust rewrite of aria2 that preserves high-value capabilities, prefers stable Rust libraries over hand-rolled protocol code, and explicitly drops obsolete legacy behavior.

Desired outcome

- Produce an execution-ready requirement baseline for what "finished enough" means for the rewrite.
- Clarify which aria2 capabilities are mandatory, optional, or intentionally out of scope.
- Clarify compatibility expectations for CLI, RPC, BitTorrent, Metalink, and downloader semantics.

Stated solution

- Keep rewriting aria2 in Rust as a new version.
- Prefer stable Rust crates and robust library-backed implementations.
- Do not spend effort on obsolete data structures or stale legacy features.
- Do support everything that is still valuable and reasonable; do not use "modernization" as an excuse to skip important capability.

Probable intent hypothesis

- The user wants a production-grade successor to aria2, not a toy clone and not a museum-quality port.
- The user wants strong practical compatibility for core workflows without inheriting legacy implementation debt.
- The user is trying to lock scope and quality bars before pushing further implementation.

Known facts and evidence

- Brownfield Rust workspace with 9 crates covering core engine, range backends, HTTP, FTP, SFTP, BT, Metalink, RPC, and CLI.
- `aria2-legacy` is vendored for comparison and contains the legacy manual plus C++ source.
- Current code uses stable Rust ecosystem libraries such as `reqwest`, `jsonrpsee`, `axum`, `redb`, `suppaftp`, `russh-sftp`, and `librqbit`.
- Repository tests currently pass across the workspace, including daemon, RPC, session restore, Metalink, SFTP, and BT integration coverage.
- Legacy RPC factory exposes 36 method names; current Rust RPC surface exposes 33 declared methods plus parity notifications.

Constraints

- Prefer library-backed implementations over custom protocol stacks.
- Avoid unnecessary support for obsolete / low-value legacy behavior.
- Preserve valuable user-facing compatibility where practical.
- Do not silently reduce scope under the banner of simplification.

Unknowns and open questions

- What exact bar defines "aria2 new version" for this project: practical successor, high-compat replacement, or near-parity rewrite?
- Which old features are explicitly considered obsolete versus still mandatory?
- Which compatibility surfaces matter most: CLI flags, JSON-RPC method parity, runtime semantics, or ecosystem-client compatibility?
- How much deviation is acceptable when upstream Rust libraries cannot express a legacy aria2 behavior exactly?
- What decisions may be made autonomously versus requiring explicit approval?

Decision-boundary unknowns

- Can OMX intentionally drop a legacy feature if a stable library does not support it?
- Can compatibility be approximate at the RPC / CLI surface, or must semantics match legacy behavior where implemented?
- Should BitTorrent be held to the same compatibility bar as HTTP/FTP/SFTP, or treated as a lower-confidence subsystem until library capability catches up?

Likely codebase touchpoints

- `crates/raria-cli/src/daemon.rs`
- `crates/raria-cli/src/bt_runtime.rs`
- `crates/raria-rpc/src/methods.rs`
- `crates/raria-bt/src/service.rs`
- `crates/raria-core/src/config.rs`
- `aria2-legacy/src/RpcMethodFactory.cc`
- `aria2-legacy/doc/manual-src/en/aria2c.rst`
