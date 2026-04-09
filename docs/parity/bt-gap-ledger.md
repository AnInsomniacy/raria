# BitTorrent Gap Ledger

> These are known incompatibilities between raria (via librqbit) and aria2 1.37.0.
> Each gap has a corresponding `#[ignore]` test in `crates/raria-bt/tests/bt_gap_ledger.rs`.
> Run `cargo test -p raria-bt -- --ignored` to see all gaps.

## BT-GAP-001: MSE/PSE Encryption

**aria2:** Supports Message Stream Encryption (BEP-7 variant) via `--bt-require-crypto` and `--bt-min-crypto-level`.

**librqbit:** No encryption support. All connections are plaintext.

**Impact:** Cannot connect to peers on private trackers that require encryption. Public trackers unaffected.

**Resolution path:** Wait for librqbit upstream implementation or fork.

---

## BT-GAP-002: WebSeed (BEP-17/BEP-19)

**aria2:** Can use HTTP/FTP URLs embedded in torrent files as additional download sources.

**librqbit:** Not supported. Upstream issue: https://github.com/ikatson/rqbit/issues/504

**Impact:** Slower downloads for torrents that include web seed URLs.

**Resolution path:** Upstream issue open, may be resolved in future librqbit versions.

---

## BT-GAP-003: Rarest-First Piece Selection

**aria2:** Uses rarest-first piece selection by default for optimal swarm health.

**librqbit:** Uses sequential piece selection only.

**Impact:** Reduced contribution to swarm health. Download still completes normally.

**Resolution path:** Accept as behavioral difference. Sequential is acceptable for most users.

---

## BT-GAP-004: Mixed HTTP+BT Source Download

**aria2:** Can download the same file simultaneously from HTTP/FTP mirrors and BitTorrent peers, with the HTTP-downloaded data being cross-seeded to the BT swarm.

**librqbit:** No support for mixed-protocol downloads of the same file.

**Impact:** Users must choose either HTTP/FTP or BT for each download. Cannot combine sources.

**Resolution path:** This is an aria2-unique feature. Accept as permanent gap.

---

## BT-GAP-005: onBtDownloadComplete Notification

**aria2:** Sends `aria2.onBtDownloadComplete` when a torrent download finishes but seeding continues, separate from `aria2.onDownloadComplete` which fires when seeding also stops.

**raria:** Not yet implemented. Requires BtService to report the downloading→seeding state transition.

**Resolution path:** Implement when BT service is wired into the main engine loop (Phase 3).
