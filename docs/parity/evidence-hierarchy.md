# Evidence Hierarchy

Every parity claim must point to evidence and must respect this order:

1. runtime evidence
2. test evidence
3. ledger index
4. README, docs, release text

Operational rules:

- When runtime and test disagree, investigate the harness first.
- When tests and ledgers disagree, update the ledger or the test mapping.
- When docs and ledgers disagree, docs lose.

This hierarchy exists to stop repository prose from outrunning the implementation.
