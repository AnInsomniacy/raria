# Native ED2K/eMule Progress

This file is the compact chronological evidence trail for
`docs/core-modernization/ed2k-native`.

## 2026-05-28 ED2K-001 verified

Changed: Created the native ED2K/eMule tracker under
`docs/core-modernization/ed2k-native`. The tracker defines aMule as the primary
behavior reference, aria2-next ED2K trackers as engineering references, and
raria-native public surfaces as the only accepted product contract. It records
the Rust library strategy, GPL isolation rule, retained downloader scope,
pruned application-shell behavior, restrained test policy, and 26 checkpoint
execution roadmap.

Verified: CSV validation passed for 54 tracker files. Stale ED2K exclusion
phrase scan passed. `git diff --check` passed. `cargo check --workspace
--locked` passed.

Remaining: Start ED2K-002 authority, license, and dependency audit.

Blocked: none.
