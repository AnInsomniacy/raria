# BitTorrent Stop-Line Ledger

This document is the repository-facing companion to the ignored parity-gap tests in `crates/raria-bt/tests/bt_gap_ledger.rs`.

Its job is simple:

- keep BitTorrent capability claims honest
- distinguish working baseline behavior from bounded parity gaps
- explain which gaps are blocked by upstream dependency limits or by explicit architecture boundaries

## Baseline Behavior Already Present

The following BitTorrent behavior is already part of the current baseline:

- magnet and torrent ingestion
- file-selection intent capture
- real BT metadata projection (`info_hash`, `torrent_name`, `announce_list`, `num_seeders`, `piece_length`, `num_pieces`)
- `Active -> Seeding -> Complete` lifecycle support
- one-shot `BtDownloadComplete`
- tracker override support
- daemon-path tracker announce coverage
- daemon-path peer projection with explicit defaults for unsupported parity-only fields

Representative anchors:

- [/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/bt_runtime.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/bt_runtime.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/facade.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/facade.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/tests/bt_tracker_smoke.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/tests/bt_tracker_smoke.rs)
- [/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/tests/options_parity.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/tests/options_parity.rs)

## Explicit Stop-Lines

### BT-GAP-001

- `gap_id`: `BT-GAP-001`
- `feature`: MSE/PSE encryption parity
- `grade`: `advanced-but-acceptable`
- `blocking_dependency`: `librqbit`
- `why_not_fixable_locally`: Peer-wire encryption support lives below `raria`'s adapter layer. Local implementation would require forking or patching the dependency, which is outside the allowed execution boundary.
- `temporary_behavior`: Torrents still work, but peers or trackers that require encrypted BT traffic are unsupported.
- `evidence`:
  - [/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs)

### BT-GAP-002

- `gap_id`: `BT-GAP-002`
- `feature`: WebSeed parity (`BEP-17` / `BEP-19`)
- `grade`: `advanced-but-acceptable`
- `blocking_dependency`: `librqbit`
- `why_not_fixable_locally`: WebSeed handling is not exposed through the current dependency path. Supporting it locally would require upstream feature work.
- `temporary_behavior`: `raria` downloads from torrent peers only; it does not consume HTTP/FTP web seeds embedded in torrent metadata.
- `evidence`:
  - [/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs)

### BT-GAP-003

- `gap_id`: `BT-GAP-003`
- `feature`: rarest-first piece-selection parity
- `grade`: `advanced-but-acceptable`
- `blocking_dependency`: `librqbit`
- `why_not_fixable_locally`: Piece scheduling is owned by the BT dependency. `raria` projects lifecycle truth, but it does not directly schedule BT pieces.
- `temporary_behavior`: Torrent downloads remain functional, but swarm-health behavior differs from aria2's rarest-first strategy.
- `evidence`:
  - [/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs)

### BT-GAP-004

- `gap_id`: `BT-GAP-004`
- `feature`: mixed range + BitTorrent downloading of the same file
- `grade`: `migration-only`
- `blocking_dependency`: `librqbit` and the current daemon architecture boundary
- `why_not_fixable_locally`: Supporting this would require deep shared-state coordination between the range executor and BT piece state over the same output file. That is outside the current architecture and would amount to a new source-graph system.
- `temporary_behavior`: Users must choose either range-based protocols or BitTorrent for a given file.
- `evidence`:
  - [/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs)

## Maintenance Rule

When a new BitTorrent parity gap is identified:

1. add or tighten a test first
2. decide whether the gap is locally fixable
3. if it is not locally fixable, add or update the stop-line entry here and keep repo-facing docs consistent
4. do not describe existing baseline behavior as “planned” simply because a separate parity gap remains
