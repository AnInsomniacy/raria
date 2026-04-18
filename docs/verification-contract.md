# Verification Contract

Repository verification is phase-aware.

Minimum repository bar:

```bash
cargo test --workspace
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Parity-specific verification additionally requires:

- generated claim inventory
- generated RPC method and notification manifests
- exported-surface policy checks
- phase-exit checks against `.omx/parity/phase-exit-policy.yaml`
- claim drift checks against `.omx/parity/claim-policy.yaml`

No phase may be reported complete if its required gate set is not green.
