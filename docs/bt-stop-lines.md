# BT Stop-Lines

BitTorrent parity is tracked explicitly.

The current known deep-water gaps are indexed by `crates/raria-bt/tests/bt_gap_ledger.rs` and machine-mapped by `.omx/parity/generated/bt-gap-capability-index.yaml`.

Until each gap is resolved or reclassified with evidence, the repository must not claim complete BT parity.

Known indexed gaps:

- None currently. `scripts/parity/build_bt_gap_index.sh` should now render an empty `entries` list.
