use std::{net::SocketAddr, time::Duration};

use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, Magnet, Session, SessionOptions,
    create_torrent,
};
use raria_core::{
    DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcValue, parse_magnet_uri,
    parse_torrent_bytes,
};
use tokio::{fs, time::timeout};

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

#[tokio::test(flavor = "multi_thread")]
async fn downloads_torrent_file_from_initial_peer() {
    let seeder_root = tempfile::tempdir().expect("seeder tempdir");
    let seed_file = seeder_root.path().join("payload.txt");
    fs::write(&seed_file, b"torrent payload from raria")
        .await
        .expect("seed file");
    let torrent = create_torrent(
        &seed_file,
        CreateTorrentOptions {
            piece_length: Some(16 * 1024),
            ..Default::default()
        },
    )
    .await
    .expect("create torrent");
    let torrent_bytes = torrent.as_bytes().expect("torrent bytes");

    let seeder_session = Session::new_with_opts(
        seeder_root.path().to_path_buf(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            listen_port_range: Some(18100..18120),
            ..Default::default()
        },
    )
    .await
    .expect("seeder session");
    let seeder_handle = seeder_session
        .add_torrent(
            AddTorrent::TorrentFileBytes(torrent_bytes.clone()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(seeder_root.path().to_string_lossy().into_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("add seeder torrent")
        .into_handle()
        .expect("seeder handle");
    wait_for_complete(seeder_handle).await;
    let peer: SocketAddr = format!(
        "127.0.0.1:{}",
        seeder_session.tcp_listen_port().expect("seeder port")
    )
    .parse()
    .expect("peer addr");

    let download_root = tempfile::tempdir().expect("download tempdir");
    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addTorrent",
            RpcValue::array([
                RpcValue::string(base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    &torrent_bytes,
                )),
                RpcValue::array([]),
                RpcValue::object([("bt-initial-peer", RpcValue::string(peer.to_string()))]),
            ]),
        ))
        .expect("add torrent")
        .as_str()
        .expect("gid")
        .to_owned();

    let config = RariaConfig {
        download_dir: download_root.path().to_path_buf(),
        ..RariaConfig::default()
    };
    timeout(
        Duration::from_secs(20),
        DownloadEngine::new(config).run_once(&mut rpc),
    )
    .await
    .expect("bt download timeout")
    .expect("bt download");

    let bytes = fs::read(download_root.path().join("payload.txt"))
        .await
        .expect("downloaded payload");
    assert_eq!(bytes, b"torrent payload from raria");

    let status = rpc
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("status");
    assert_eq!(
        status.get("status").and_then(RpcValue::as_str),
        Some("complete")
    );
    assert_eq!(
        status.get("completedLength").and_then(RpcValue::as_str),
        Some("26")
    );

    seeder_session.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_magnet_from_initial_peer() {
    let seeder_root = tempfile::tempdir().expect("seeder tempdir");
    let seed_file = seeder_root.path().join("magnet-payload.txt");
    let payload = b"magnet payload from raria";
    fs::write(&seed_file, payload).await.expect("seed file");
    let torrent = create_torrent(
        &seed_file,
        CreateTorrentOptions {
            piece_length: Some(16 * 1024),
            ..Default::default()
        },
    )
    .await
    .expect("create torrent");
    let torrent_bytes = torrent.as_bytes().expect("torrent bytes");
    let magnet = Magnet::from_id20(torrent.info_hash(), Vec::new(), None).to_string();

    let seeder_session = Session::new_with_opts(
        seeder_root.path().to_path_buf(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            listen_port_range: Some(18120..18140),
            ..Default::default()
        },
    )
    .await
    .expect("seeder session");
    let seeder_handle = seeder_session
        .add_torrent(
            AddTorrent::TorrentFileBytes(torrent_bytes),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(seeder_root.path().to_string_lossy().into_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("add seeder torrent")
        .into_handle()
        .expect("seeder handle");
    wait_for_complete(seeder_handle).await;
    let peer = format!(
        "127.0.0.1:{}",
        seeder_session.tcp_listen_port().expect("seeder port")
    );

    let download_root = tempfile::tempdir().expect("download tempdir");
    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(magnet)]),
                RpcValue::object([("bt-initial-peer", RpcValue::string(peer))]),
            ]),
        ))
        .expect("add magnet")
        .as_str()
        .expect("gid")
        .to_owned();

    let config = RariaConfig {
        download_dir: download_root.path().to_path_buf(),
        ..RariaConfig::default()
    };
    timeout(
        Duration::from_secs(20),
        DownloadEngine::new(config).run_once(&mut rpc),
    )
    .await
    .expect("magnet download timeout")
    .expect("magnet download");

    let bytes = fs::read(download_root.path().join("magnet-payload.txt"))
        .await
        .expect("downloaded payload");
    assert_eq!(bytes, payload);

    let status = rpc
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("status");
    assert_eq!(
        status.get("status").and_then(RpcValue::as_str),
        Some("complete")
    );
    assert_eq!(
        status.get("completedLength").and_then(RpcValue::as_str),
        Some("25")
    );

    seeder_session.stop().await;
}

async fn wait_for_complete(handle: std::sync::Arc<librqbit::ManagedTorrent>) {
    timeout(Duration::from_secs(10), handle.wait_until_completed())
        .await
        .expect("seeder completion timeout")
        .expect("seeder complete");
}
