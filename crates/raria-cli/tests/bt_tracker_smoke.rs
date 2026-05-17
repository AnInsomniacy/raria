use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use base64::Engine;
use librqbit::{
    AddTorrent as RqbitAddTorrent, AddTorrentOptions as RqbitAddTorrentOptions,
    CreateTorrentOptions, Session as RqbitSession, SessionOptions as RqbitSessionOptions,
    create_torrent,
};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static BT_DAEMON_SMOKE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

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

fn directory_has_state(path: &std::path::Path) -> bool {
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .any(|metadata| metadata.is_file() && metadata.len() > 0)
}

async fn wait_for_rpc_ready_with_child(port: u16, child: &mut ChildGuard) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(120);
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
            return Err(format!(
                "daemon RPC server did not become ready on port {port}"
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn spawn_ready_daemon(
    download_dir: &std::path::Path,
    session_file: &std::path::Path,
) -> (ChildGuard, u16) {
    spawn_ready_daemon_with_args(download_dir, session_file, &[]).await
}

async fn spawn_ready_daemon_with_args(
    download_dir: &std::path::Path,
    session_file: &std::path::Path,
    extra_args: &[&str],
) -> (ChildGuard, u16) {
    for _ in 0..8 {
        let rpc_port = allocate_port();
        let mut cmd = Command::new(cargo_bin("raria"));
        cmd.arg("daemon")
            .arg("-d")
            .arg(download_dir)
            .arg("--rpc-port")
            .arg(rpc_port.to_string())
            .arg("--session-file")
            .arg(session_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for arg in extra_args {
            cmd.arg(arg);
        }
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

async fn create_native_bt_task(
    client: &reqwest::Client,
    port: u16,
    download_dir: &std::path::Path,
    fixture: &BtSeedFixture,
    seeding_stop_after_minutes: Option<u64>,
) -> String {
    let mut bt = serde_json::json!({
        "trackerUris": [fixture.tracker_url],
    });
    if let Some(minutes) = seeding_stop_after_minutes {
        bt["seeding"] = serde_json::json!({
            "stopAfterMinutes": minutes
        });
    }

    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("torrent:base64:{}", fixture.torrent_b64)],
            "downloadDir": download_dir,
            "bt": bt
        }))
        .send()
        .await
        .expect("send native BT task create")
        .json()
        .await
        .expect("parse native BT task create response");

    created["taskId"]
        .as_str()
        .unwrap_or_else(|| panic!("native BT task should be accepted: {created}"))
        .to_string()
}

async fn request_native_shutdown(client: &reqwest::Client, port: u16) {
    let response = client
        .post(format!("http://127.0.0.1:{port}/api/v1/daemon/shutdown"))
        .send()
        .await
        .expect("send native shutdown");
    assert!(
        response.status().is_success(),
        "native shutdown should return success, got {}",
        response.status()
    );
    let body: serde_json::Value = response
        .json()
        .await
        .expect("parse native shutdown response");
    assert_eq!(body["status"], "shuttingDown");
    assert!(body.get("jsonrpc").is_none());
    assert!(body.get("result").is_none());
}

async fn wait_for_child_exit_after_native_shutdown(child: &mut ChildGuard) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "daemon exited unsuccessfully: {status}");
                return;
            }
            Ok(None) => {
                assert!(
                    Instant::now() < deadline,
                    "daemon did not exit after native shutdown request"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("failed waiting for daemon exit: {error}"),
        }
    }
}

async fn wait_for_native_bt_tracker_announce(
    client: &reqwest::Client,
    port: u16,
    task_id: &str,
    fixture: &BtSeedFixture,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let task_resp: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
            .send()
            .await
            .expect("send native task detail")
            .json()
            .await
            .expect("parse native task detail");
        let tracker_requests = fixture.tracker.received_requests().await;
        if tracker_requests
            .as_ref()
            .is_some_and(|requests| !requests.is_empty())
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "BT daemon never announced to tracker\ntask: {task_resp}\ntracker_requests: {tracker_requests:#?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn lock_bt_daemon_smoke_lane() -> tokio::sync::MutexGuard<'static, ()> {
    BT_DAEMON_SMOKE_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

struct BtSeedFixture {
    tracker_url: String,
    torrent_b64: String,
    tracker: MockServer,
    _seed_root: tempfile::TempDir,
    _seed_session: std::sync::Arc<RqbitSession>,
}

async fn spawn_bt_seed_fixture_with_payload(payload: Vec<u8>) -> BtSeedFixture {
    let seed_root = tempdir().expect("seed tempdir");
    let seed_file = seed_root.path().join("seed.bin");
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
        torrent_b64: base64::engine::general_purpose::STANDARD
            .encode(torrent.as_bytes().expect("torrent bytes")),
        tracker,
        _seed_root: seed_root,
        _seed_session: session,
    }
}

async fn spawn_bt_seed_fixture() -> BtSeedFixture {
    spawn_bt_seed_fixture_with_payload(b"raria-bt-seed-payload".to_vec()).await
}

#[tokio::test]
async fn daemon_binds_bt_fastresume_state_to_native_session_path() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_seed_fixture().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("bt-fastresume.session.redb");
    let expected_state_dir = temp.path().join("bt-fastresume.session.redb.bt-session");
    let old_download_scoped_dir = temp.path().join(".raria-bt-session");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;
    let client = reqwest::Client::new();

    let task_id = create_native_bt_task(&client, rpc_port, temp.path(), &fixture, None).await;
    wait_for_native_bt_tracker_announce(&client, rpc_port, &task_id, &fixture).await;

    request_native_shutdown(&client, rpc_port).await;
    wait_for_child_exit_after_native_shutdown(&mut child).await;

    assert!(
        directory_has_state(&expected_state_dir),
        "BT fastresume state should be persisted under the native session-derived directory"
    );
    assert!(
        !directory_has_state(&old_download_scoped_dir),
        "BT fastresume state should not use the old download-scoped default directory"
    );
}

#[tokio::test]
async fn daemon_bt_tracker_option_announces_to_tracker_on_real_daemon_path() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_seed_fixture().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("bt-download.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;
    let client = reqwest::Client::new();

    let task_id = create_native_bt_task(&client, rpc_port, temp.path(), &fixture, None).await;

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let task_resp: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{rpc_port}/api/v1/tasks/{task_id}"
            ))
            .send()
            .await
            .expect("send native task detail")
            .json()
            .await
            .expect("parse native task detail");

        let tracker_requests = fixture.tracker.received_requests().await;
        if let Some(requests) = tracker_requests.as_ref() {
            if !requests.is_empty() {
                assert_eq!(task_resp["lifecycle"].as_str(), Some("running"));
                let request_url = &requests[0].url;
                let query = request_url.query().expect("tracker query string");
                assert!(
                    query.contains("event=started"),
                    "tracker query should announce start: {query}"
                );
                assert!(
                    query.contains("left=21"),
                    "tracker query should advertise remaining bytes: {query}"
                );
                break;
            }
        }

        if Instant::now() >= deadline {
            panic!(
                "BT daemon never announced to tracker on daemon path: {task_resp}\ntracker_requests: {tracker_requests:#?}"
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    request_native_shutdown(&client, rpc_port).await;
    wait_for_child_exit_after_native_shutdown(&mut child).await;
}

#[tokio::test]
async fn daemon_shutdown_persists_bt_dht_snapshot_before_periodic_dump_window() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_seed_fixture().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("bt-dht.session.redb");
    let dht_config_file = temp.path().join("bt-dht.json");
    let dht_config_arg = dht_config_file.to_string_lossy().to_string();
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--bt-dht-config-file", &dht_config_arg],
    )
    .await;
    let client = reqwest::Client::new();

    let _task_id = create_native_bt_task(&client, rpc_port, temp.path(), &fixture, None).await;
    assert!(
        !dht_config_file.exists(),
        "fresh daemon run should not persist DHT state before shutdown is requested"
    );

    let shutdown_started_at = Instant::now();
    request_native_shutdown(&client, rpc_port).await;
    wait_for_child_exit_after_native_shutdown(&mut child).await;

    assert!(
        shutdown_started_at.elapsed() < Duration::from_secs(3),
        "shutdown-driven DHT snapshot should land before the upstream 3s periodic dump fallback"
    );

    let dht_bytes = std::fs::read(&dht_config_file)
        .expect("DHT config file should be written during daemon shutdown");
    let dht_json: serde_json::Value =
        serde_json::from_slice(&dht_bytes).expect("parse persisted DHT JSON");
    assert!(
        dht_json.get("addr").is_some(),
        "persisted DHT JSON should include the listen address"
    );
    assert!(
        dht_json.get("table").is_some(),
        "persisted DHT JSON should include the routing table"
    );
    assert!(
        dht_json.get("peer_store").is_some(),
        "persisted DHT JSON should include the peer store field from the upstream writer"
    );
}

#[tokio::test]
async fn daemon_log_file_contains_structured_bt_lifecycle_events() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_seed_fixture().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("bt-log.session.redb");
    let log_path = temp.path().join("bt.log");
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--log", log_path.to_str().unwrap()],
    )
    .await;
    let client = reqwest::Client::new();

    let task_id = create_native_bt_task(&client, rpc_port, temp.path(), &fixture, Some(1)).await;

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let task_resp: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{rpc_port}/api/v1/tasks/{task_id}"
            ))
            .send()
            .await
            .expect("send native task detail")
            .json()
            .await
            .expect("parse native task detail");

        let tracker_requests = fixture.tracker.received_requests().await;
        if tracker_requests
            .as_ref()
            .is_some_and(|requests| !requests.is_empty())
            && task_resp["lifecycle"].as_str() == Some("running")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "BT daemon never reached an announced running state: {task_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    request_native_shutdown(&client, rpc_port).await;
    wait_for_child_exit_after_native_shutdown(&mut child).await;

    let entries = std::fs::read_to_string(&log_path)
        .expect("read log file")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSON line"))
        .collect::<Vec<_>>();

    assert!(
        entries.iter().any(|entry| {
            entry["target"] == "raria::bt"
                && entry["message"] == "BT download started"
                && entry["fields"]["gid"].as_str().is_some()
        }),
        "structured log should capture BT start events"
    );
    assert!(
        entries.iter().any(|entry| {
            entry["target"] == "raria::bt"
                && entry["message"] == "BT download cancelled"
                && entry["fields"]["gid"].as_str().is_some()
        }),
        "structured log should capture BT shutdown cancellation events"
    );
}
