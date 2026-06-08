use raria_core::{parse_magnet_uri, parse_torrent_bytes};

#[test]
fn parses_single_file_torrent_metadata() {
    let torrent = b"d8:announce31:http://tracker.example/announce4:infod6:lengthi14e4:name8:file.txt12:piece lengthi16384e6:pieces20:12345678901234567890ee";

    let meta = parse_torrent_bytes(torrent).expect("torrent metadata");

    assert_eq!(meta.name, "file.txt");
    assert_eq!(meta.total_length, 14);
    assert_eq!(meta.files.len(), 1);
    assert_eq!(meta.files[0].path, "file.txt");
    assert_eq!(meta.files[0].length, 14);
    assert_eq!(meta.piece_length, 16384);
    assert_eq!(
        meta.info_hash_hex,
        "9d8cd776fc2f80d08eee2de831b139010d4b033f"
    );
    assert_eq!(
        meta.announce.as_deref(),
        Some("http://tracker.example/announce")
    );
}

#[test]
fn parses_magnet_uri_for_btih_and_display_name() {
    let magnet = parse_magnet_uri(
        "magnet:?xt=urn:btih:9d8cd776fc2f80d08eee2de831b139010d4b033f&dn=file.txt&tr=http%3A%2F%2Ftracker.example%2Fannounce",
    )
    .expect("magnet");

    assert_eq!(
        magnet.info_hash_hex,
        "9d8cd776fc2f80d08eee2de831b139010d4b033f"
    );
    assert_eq!(magnet.name.as_deref(), Some("file.txt"));
    assert_eq!(magnet.trackers, vec!["http://tracker.example/announce"]);
}
