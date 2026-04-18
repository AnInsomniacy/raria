# Parity Rubric

`raria` does not use percentage theater as a substitute for evidence.

The allowed parity classes are:

- `MustMatch`
  - migration-critical behavior
  - release cannot claim success while key `MustMatch` capability gaps remain open
- `AcceptableDelta`
  - intentionally different behavior with explicit user impact and evidence
- `ExplicitlyExcluded`
  - out-of-scope historical behavior already excluded by product decision
- `PlannedGap`
  - still in scope, not yet complete, and explicitly owned

Evidence precedence is:

1. runtime evidence
2. test evidence
3. ledger index
4. docs or release text

No document is allowed to override runtime or test truth.
