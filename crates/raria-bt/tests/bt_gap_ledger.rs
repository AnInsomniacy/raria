// BitTorrent gap tests.
//
// These tests document known incompatibilities between raria (via librqbit)
// and aria2 1.37.0's BitTorrent implementation.
//
// Run `cargo test -- --ignored` to see all documented gaps.
// When a gap is resolved upstream, remove the #[ignore] attribute.

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "BT-GAP-001: librqbit does not support MSE/PSE (Message Stream Encryption). \
                aria2 supports both forced and opportunistic encryption via --bt-require-crypto \
                and --bt-min-crypto-level. This is required for trackers that mandate encryption."]
    fn bt_mse_pse_encryption() {
        // librqbit has no encryption support as of 2026-04.
        // Upstream issue: not yet filed.
        // Impact: Cannot connect to peers on private trackers requiring encryption.
        panic!("not implemented");
    }

    #[test]
    #[ignore = "BT-GAP-002: librqbit does not support WebSeed (BEP-17 and BEP-19). \
                aria2 can use HTTP/FTP URLs embedded in torrents as additional download sources. \
                Upstream issue: https://github.com/ikatson/rqbit/issues/504"]
    fn bt_webseed_bep17_bep19() {
        // WebSeed allows using HTTP mirrors alongside BitTorrent peers.
        // Impact: Slower downloads for torrents that include web seeds.
        panic!("not implemented");
    }

    #[test]
    fn bt_rarest_first_piece_selection_contract() {
        let ordered = librqbit::parity_contract_sort_piece_candidates(
            librqbit::PieceSelectionStrategy::RarestFirst,
            &[(0, 5), (1, 1), (2, 3)],
        );
        assert_eq!(
            ordered,
            vec![1, 2, 0],
            "rarest-first must prioritize lower-availability pieces"
        );
    }

    #[test]
    #[ignore = "BT-GAP-004: Mixed HTTP/FTP/SFTP + BitTorrent source for same file is not supported. \
                aria2 can download a file simultaneously from HTTP mirrors and BitTorrent peers, \
                cross-seeding the HTTP-downloaded data to the BT swarm. This is an aria2-unique \
                feature that requires deep integration between protocol backends."]
    fn bt_mixed_protocol_source_download() {
        // This would require range-backend and bt-backend to share the same output file
        // and coordinate piece completion state. librqbit's API does not support this.
        // Impact: Users must choose either HTTP or BT for each download, not both simultaneously.
        panic!("not implemented");
    }
}
