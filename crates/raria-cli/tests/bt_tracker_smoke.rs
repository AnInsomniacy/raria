use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use librqbit::{
    AddTorrent as RqbitAddTorrent, AddTorrentOptions as RqbitAddTorrentOptions, CreateTorrentOptions,
    Session as RqbitSession, SessionOptions as RqbitSessionOptions, create_torrent,
};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cargo_bin(name: &str) -> String {
    std::env::var(format!("CARGO_BIN_EXE_{name}")).expect("cargo should provide binary path")
}

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn allocate_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

async fn wait_for_rpc_ready_with_child(port: u16, child: &mut ChildGuard) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let client = reqwest::Client::new();

    loop {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.getVersion",
            "params": [],
        });

        if let Ok(resp) = client
            .post(format!("http://127.0.0.1:{port}"))
            .json(&body)
            .send()
            .await
        {
            if resp.status().is_success() {
                return Ok(());
            }
        }

        match child.child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut handle) = child.child.stdout.take() {
                    let _ = handle.read_to_string(&mut stdout);
                }
                let mut stderr = String::new();
                if let Some(mut handle) = child.child.stderr.take() {
                    let _ = handle.read_to_string(&mut stderr);
                }
                return Err(format!(
                    "daemon exited before RPC became ready on port {port}: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                ));
            }
            Ok(None) => {}
            Err(error) => return Err(format!("failed checking daemon process state: {error}")),
        }

        if Instant::now() >= deadline {
            return Err(format!("daemon RPC server did not become ready on port {port}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn spawn_ready_daemon(download_dir: &std::path::Path, session_file: &std::path::Path) -> (ChildGuard, u16) {
    for _ in 0..8 {
        let rpc_port = allocate_port();
        let mut cmd = Command::new(cargo_bin("raria"));
        cmd
            .arg("daemon")
            .arg("-d")
            .arg(download_dir)
            .arg("--rpc-port")
            .arg(rpc_port.to_string())
            .arg("--session-file")
            .arg(session_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd.spawn().expect("spawn daemon");
        let mut child = ChildGuard { child };

        match wait_for_rpc_ready_with_child(rpc_port, &mut child).await {
            Ok(()) => return (child, rpc_port),
            Err(message) if message.contains("failed to bind RPC server") => continue,
            Err(message) => panic!("{message}"),
        }
    }

    panic!("failed to start daemon on a free RPC port after multiple attempts");
}

struct BtSeedFixture {
    tracker_url: String,
    torrent_b64: String,
    tracker: MockServer,
    _seed_root: tempfile::TempDir,
    _seed_session: std::sync::Arc<RqbitSession>,
}

async fn spawn_bt_seed_fixture() -> BtSeedFixture {
    let seed_root = tempdir().expect("seed tempdir");
    let seed_file = seed_root.path().join("seed.bin");
    let payload = b"raria-bt-seed-payload".to_vec();
    std::fs::write(&seed_file, &payload).expect("write seed payload");

    let torrent = create_torrent(
        &seed_file,
        CreateTorrentOptions {
            piece_length: Some(1024),
            ..Default::default()
        },
    )
    .await
    .expect("create torrent");

    let listen_port = allocate_port();
    let session = RqbitSession::new_with_opts(
        seed_root.path().to_path_buf(),
        RqbitSessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            listen_port_range: Some(listen_port..(listen_port + 1)),
            enable_upnp_port_forwarding: false,
            ..Default::default()
        },
    )
    .await
    .expect("create seed session");

    session
        .add_torrent(
            RqbitAddTorrent::from_bytes(torrent.as_bytes().expect("torrent bytes")),
            Some(RqbitAddTorrentOptions {
                paused: false,
                output_folder: Some(seed_root.path().to_string_lossy().to_string()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .expect("add seed torrent")
        .into_handle()
        .expect("seed handle")
        .wait_until_completed()
        .await
        .expect("seed complete");

    let peer_port = session.tcp_listen_port().expect("seed listen port");
    let tracker = MockServer::start().await;
    let mut tracker_body = b"d8:intervali60e5:peers6:".to_vec();
    tracker_body.extend_from_slice(&[127, 0, 0, 1]);
    tracker_body.extend_from_slice(&peer_port.to_be_bytes());
    tracker_body.extend_from_slice(b"e");

    Mock::given(method("GET"))
        .and(path("/announce"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tracker_body))
        .mount(&tracker)
        .await;

    BtSeedFixture {
        tracker_url: format!("{}/announce", tracker.uri()),
        torrent_b64: base64::engine::general_purpose::STANDARD.encode(
            torrent.as_bytes().expect("torrent bytes"),
        ),
        tracker,
        _seed_root: seed_root,
        _seed_session: session,
    }
}

#[tokio::test]
async fn daemon_bt_tracker_option_announces_to_tracker_on_real_daemon_path() {
    let fixture = spawn_bt_seed_fixture().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("bt-download.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;
    let client = reqwest::Client::new();

    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addTorrent",
            "params": [
                fixture.torrent_b64,
                [],
                { "bt-tracker": fixture.tracker_url }
            ],
        }))
        .send()
        .await
        .expect("send addTorrent")
        .json()
        .await
        .expect("parse addTorrent response");
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status_resp: serde_json::Value = client
            .post(format!("http://127.0.0.1:{rpc_port}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "aria2.tellStatus",
                "params": [gid.clone()],
            }))
            .send()
            .await
            .expect("send tellStatus")
            .json()
            .await
            .expect("parse tellStatus");

        let tracker_requests = fixture.tracker.received_requests().await;
        if let Some(requests) = tracker_requests.as_ref() {
            if !requests.is_empty() {
                assert_eq!(status_resp["result"]["status"].as_str(), Some("active"));
                let request_url = &requests[0].url;
                let query = request_url.query().expect("tracker query string");
                assert!(query.contains("event=started"), "tracker query should announce start: {query}");
                assert!(query.contains("left=21"), "tracker query should advertise remaining bytes: {query}");
                break;
            }
        }

        if Instant::now() >= deadline {
            panic!(
                "BT daemon never announced to tracker on daemon path: {status_resp}\ntracker_requests: {tracker_requests:#?}"
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "aria2.shutdown",
            "params": [],
        }))
        .send()
        .await
        .expect("send shutdown")
        .json()
        .await
        .expect("parse shutdown response");
    assert_eq!(shutdown_resp["result"], "OK");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "daemon exited unsuccessfully: {status}");
                break;
            }
            Ok(None) => {
                assert!(Instant::now() < deadline, "daemon did not exit after shutdown RPC");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("failed waiting for daemon exit: {error}"),
        }
    }
}
