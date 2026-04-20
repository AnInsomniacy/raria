Metadata

- Profile: standard
- Context type: brownfield
- Status: in progress
- Current ambiguity: 0.23
- Threshold: 0.20
- Context snapshot: `.omx/context/rust-aria2-rewrite-alignment-20260420T095658Z.md`

Working draft

- Intent:
  - Build a more modern, stable, maintainable Rust successor to aria2.
  - Practical engineering quality takes precedence over museum-grade legacy reproduction.
  - Feasible valuable capability should still be implemented; modernization must not be used as cover for laziness.
- Desired Outcome: pending
- In-Scope:
  - HTTP / FTP / SFTP / RPC mainline downloader behavior should remain strong.
  - Valuable feasible capability should still be implemented even when modernization is the primary goal.
- Out-of-Scope / Non-goals:
  - It is acceptable to cut deep, old, high-cost features in BitTorrent, Metalink, advanced mirror behavior, and niche authentication areas, if they are genuinely low-value.
- Decision Boundaries:
  - Prefer stable Rust libraries first.
  - If a valuable important capability is not available from stable upstream libraries, implement it locally.
  - Do not vendor-fork or clone-and-modify third-party dependencies as the primary strategy.
- Constraints:
  - Prefer stable Rust libraries.
  - Avoid obsolete low-value legacy behavior.
  - Do not skip important capability without justification.
  - No vendored dependency forks as a shortcut around library limitations.
- Acceptance Criteria: pending
- Pressure-pass findings:
  - The user's first answer already narrowed the core objective away from full legacy reenactment.
  - The unresolved pressure point is the definition of "obsolete" versus "still valuable enough that we must support it."
- Brownfield evidence:
  - Current workspace already implements real HTTP/FTP/SFTP/BT/Metalink/RPC subsystems.
  - Current codebase leans heavily on stable crates instead of custom protocol implementations.
  - Current repository has broad passing test coverage across these subsystems.

Clarity breakdown

| Dimension | Score |
|---|---:|
| Intent | 0.85 |
| Outcome | 0.48 |
| Scope | 0.75 |
| Constraints | 0.95 |
| Success | 0.38 |
| Context | 0.85 |

Readiness gates

- Non-goals: partially resolved
- Decision Boundaries: substantially resolved
- Pressure pass complete: no
