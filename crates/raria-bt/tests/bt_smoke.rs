use anyhow::{Context, Result};
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, Session, SessionOptions, create_torrent,
};
use raria_bt::service::{BtService, BtServiceConfig, BtSource};
use raria_core::job::Gid;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, sleep, timeout};

fn make_payload(size: usize) -> Vec<u8> {
    (0..size).map(|idx| ((idx * 31) % 251) as u8).collect()
}

fn write_fixture(path: &Path, size: usize) -> Vec<u8> {
    let data = make_payload(size);
    fs::write(path, &data).expect("write BT fixture");
    data
}

fn reserve_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("ephemeral port addr").port();
    drop(listener);
    port
}

async fn spawn_socks5_proxy(connect_count: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind socks5 proxy");
    let addr = listener.local_addr().expect("socks5 addr");

    tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            let connect_count = Arc::clone(&connect_count);
            tokio::spawn(async move {
                let mut greeting = [0u8; 2];
                downstream
                    .read_exact(&mut greeting)
                    .await
                    .expect("read greeting");
                let mut methods = vec![0u8; greeting[1] as usize];
                downstream
                    .read_exact(&mut methods)
                    .await
                    .expect("read methods");
                downstream
                    .write_all(&[0x05, 0x00])
                    .await
                    .expect("write method select");

                let mut req = [0u8; 4];
                downstream
                    .read_exact(&mut req)
                    .await
                    .expect("read request header");
                let target = match req[3] {
                    0x01 => {
                        let mut ipv4 = [0u8; 4];
                        downstream.read_exact(&mut ipv4).await.expect("read ipv4");
                        let mut port = [0u8; 2];
                        downstream.read_exact(&mut port).await.expect("read port");
                        format!(
                            "{}.{}.{}.{}:{}",
                            ipv4[0],
                            ipv4[1],
                            ipv4[2],
                            ipv4[3],
                            u16::from_be_bytes(port)
                        )
                    }
                    0x03 => {
                        let mut len = [0u8; 1];
                        downstream
                            .read_exact(&mut len)
                            .await
                            .expect("read host len");
                        let mut host = vec![0u8; len[0] as usize];
                        downstream.read_exact(&mut host).await.expect("read host");
                        let mut port = [0u8; 2];
                        downstream.read_exact(&mut port).await.expect("read port");
                        format!(
                            "{}:{}",
                            String::from_utf8(host).expect("utf8 host"),
                            u16::from_be_bytes(port)
                        )
                    }
                    other => panic!("unsupported socks atyp: {other}"),
                };

                connect_count.fetch_add(1, Ordering::SeqCst);
                let mut upstream = TcpStream::connect(target).await.expect("connect upstream");
                downstream
                    .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                    .await
                    .expect("write connect reply");
                tokio::io::copy_bidirectional(&mut downstream, &mut upstream)
                    .await
                    .expect("proxy relay");
            });
        }
    });

    format!("socks5://{addr}")
}

struct SeedFixture {
    torrent_bytes: Vec<u8>,
    payload: Vec<u8>,
    output_name: String,
    seed_addr: SocketAddr,
    _session: Arc<Session>,
    _source_root: tempfile::TempDir,
}

async fn start_seed_fixture_with_initial_peers(
    payload_len: usize,
    initial_peers: Option<Vec<SocketAddr>>,
) -> Result<SeedFixture> {
    let source_root = tempdir().context("source tempdir")?;
    let session_dir = tempdir().context("session tempdir")?;
    let output_name = "fixture.bin".to_string();
    let source_file = source_root.path().join(&output_name);
    let payload = write_fixture(&source_file, payload_len);
    fs::write(
        source_root.path().join("extra.bin"),
        make_payload(256 * 1024),
    )
    .expect("write extra payload");

    let torrent = create_torrent(
        source_root.path(),
        CreateTorrentOptions {
            piece_length: Some(16 * 1024),
            ..Default::default()
        },
    )
    .await
    .context("create torrent")?;
    let torrent_bytes = torrent.as_bytes().context("torrent bytes")?.to_vec();

    let listen_port = reserve_port();
    let session = Session::new_with_opts(
        session_dir.path().to_path_buf(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            listen_port_range: Some(listen_port..(listen_port + 1)),
            enable_upnp_port_forwarding: false,
            ..Default::default()
        },
    )
    .await
    .context("create seed session")?;

    session
        .add_torrent(
            AddTorrent::from_bytes(torrent.as_bytes().context("torrent bytes")?),
            Some(AddTorrentOptions {
                paused: false,
                output_folder: Some(source_root.path().to_string_lossy().into_owned()),
                initial_peers,
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .context("add seed torrent")?
        .into_handle()
        .context("seed torrent should create handle")?
        .wait_until_completed()
        .await
        .context("wait for seed completion")?;

    let seed_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        session.tcp_listen_port().context("seed listen port")?,
    );

    Ok(SeedFixture {
        torrent_bytes,
        payload,
        output_name,
        seed_addr,
        _session: session,
        _source_root: source_root,
    })
}

async fn start_seed_fixture(payload_len: usize) -> Result<SeedFixture> {
    start_seed_fixture_with_initial_peers(payload_len, None).await
}

async fn wait_for_bt_completion(
    service: &BtService,
    gid: Gid,
    torrent_bytes: Vec<u8>,
) -> raria_bt::service::BtHandle {
    let handle = service
        .add(BtSource::TorrentBytes(torrent_bytes), gid, None, None)
        .await
        .expect("add torrent to BtService");

    timeout(Duration::from_secs(60), async {
        loop {
            let status = service.status(&handle).await.expect("bt status");
            if status.is_complete {
                return;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("BT completion timeout");

    handle
}

async fn wait_for_partial_bt_download(
    service: &BtService,
    gid: Gid,
    torrent_bytes: Vec<u8>,
) -> (raria_bt::service::BtHandle, u64) {
    let handle = service
        .add(BtSource::TorrentBytes(torrent_bytes), gid, None, None)
        .await
        .expect("add torrent to BtService");

    let downloaded = timeout(Duration::from_secs(60), async {
        loop {
            let status = service.status(&handle).await.expect("bt status");
            if status.total_size > 0 && status.downloaded > 512 * 1024 && !status.is_complete {
                return status.downloaded;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("BT partial download timeout");

    (handle, downloaded)
}

fn persistence_dir_has_state(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .any(|metadata| metadata.is_file() && metadata.len() > 0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn bt_service_downloads_real_torrent_from_seed_peer() {
    let seed = start_seed_fixture(8 * 1024 * 1024)
        .await
        .expect("seed fixture");
    let download_dir = tempdir().expect("download tempdir");
    let service = BtService::with_config(
        download_dir.path().to_path_buf(),
        BtServiceConfig {
            disable_dht: true,
            disable_dht_persistence: true,
            initial_peers: Some(vec![seed.seed_addr]),
            ..Default::default()
        },
    )
    .expect("create bt service");

    let handle =
        wait_for_bt_completion(&service, Gid::from_raw(1), seed.torrent_bytes.clone()).await;

    let files = service.file_list(&handle).await.expect("bt file list");
    let fixture_entry = files
        .iter()
        .find(|entry| entry.path.as_os_str() == std::ffi::OsStr::new(&seed.output_name))
        .expect("fixture file entry");
    assert_eq!(fixture_entry.size, seed.payload.len() as u64);
    assert_eq!(fixture_entry.completed_length, seed.payload.len() as u64);
    assert!(fixture_entry.selected);

    assert_eq!(
        fs::read(download_dir.path().join(&seed.output_name)).expect("read downloaded torrent"),
        seed.payload
    );

    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn bt_service_completes_peer_download_through_socks5_proxy() {
    let seed = start_seed_fixture(4 * 1024 * 1024)
        .await
        .expect("seed fixture");
    let download_dir = tempdir().expect("download tempdir");
    let proxy_connects = Arc::new(AtomicUsize::new(0));
    let proxy_url = spawn_socks5_proxy(Arc::clone(&proxy_connects)).await;

    let service = BtService::with_config(
        download_dir.path().to_path_buf(),
        BtServiceConfig {
            socks_proxy_url: Some(proxy_url),
            disable_dht: true,
            disable_dht_persistence: true,
            dht_config_filename: None,
            initial_peers: Some(vec![seed.seed_addr]),
        },
    )
    .expect("create bt service");

    let handle =
        wait_for_bt_completion(&service, Gid::from_raw(2), seed.torrent_bytes.clone()).await;

    let files = service.file_list(&handle).await.expect("bt file list");
    let fixture_entry = files
        .iter()
        .find(|entry| entry.path.as_os_str() == std::ffi::OsStr::new(&seed.output_name))
        .expect("fixture file entry");
    assert_eq!(fixture_entry.size, seed.payload.len() as u64);
    assert_eq!(fixture_entry.completed_length, seed.payload.len() as u64);
    assert!(fixture_entry.selected);

    assert_eq!(
        fs::read(download_dir.path().join(&seed.output_name)).expect("read proxied torrent"),
        seed.payload
    );

    assert!(
        proxy_connects.load(Ordering::SeqCst) >= 1,
        "expected BT peer traffic to traverse the SOCKS5 proxy"
    );

    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn bt_service_persists_fastresume_state_and_restores_progress_after_restart() {
    let seed = start_seed_fixture(64 * 1024 * 1024)
        .await
        .expect("seed fixture");
    let download_dir = tempdir().expect("download tempdir");
    let config = BtServiceConfig {
        disable_dht: true,
        disable_dht_persistence: true,
        dht_config_filename: None,
        initial_peers: Some(vec![seed.seed_addr]),
        ..Default::default()
    };

    let service = BtService::with_config(download_dir.path().to_path_buf(), config.clone())
        .expect("create bt service");
    let (_handle, partial_downloaded) =
        wait_for_partial_bt_download(&service, Gid::from_raw(3), seed.torrent_bytes.clone()).await;
    service.shutdown().await;

    let persistence_dir = download_dir.path().join(".raria-bt-session");
    timeout(Duration::from_secs(10), async {
        loop {
            if persistence_dir_has_state(&persistence_dir) {
                return;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("fastresume persistence dir should contain state after shutdown");

    let resumed_service = BtService::with_config(download_dir.path().to_path_buf(), config)
        .expect("create resumed bt service");
    let resumed_handle = resumed_service
        .add(
            BtSource::TorrentBytes(seed.torrent_bytes.clone()),
            Gid::from_raw(4),
            None,
            None,
        )
        .await
        .expect("re-add torrent after restart");

    let resumed_downloaded = timeout(Duration::from_secs(10), async {
        loop {
            let status = resumed_service
                .status(&resumed_handle)
                .await
                .expect("resumed bt status");
            if status.downloaded > 0 {
                return status.downloaded;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("resumed torrent should surface preserved progress");
    assert!(
        resumed_downloaded > 0 && partial_downloaded > 0,
        "fastresume path should preserve non-zero progress across restart"
    );

    resumed_service.shutdown().await;
}
