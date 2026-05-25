use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use futures::StreamExt;
use librqbit::{
    AddTorrent as RqbitAddTorrent, AddTorrentOptions as RqbitAddTorrentOptions,
    CreateTorrentOptions, Session as RqbitSession, SessionOptions as RqbitSessionOptions,
    create_torrent,
};
use raria_core::native::TaskId;
use raria_core::persist::Store;
use tempfile::tempdir;
use tokio::net::UdpSocket;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

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

async fn wait_for_native_api_ready(port: u16, child: &mut ChildGuard) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let client = reqwest::Client::new();

    loop {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{port}/api/v1/health"))
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
                    "daemon exited before native API became ready on port {port}: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                ));
            }
            Ok(None) => {}
            Err(error) => return Err(format!("failed checking daemon process state: {error}")),
        }

        if Instant::now() >= deadline {
            return Err(format!("native API did not become ready on port {port}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn spawn_native_daemon(
    download_dir: &std::path::Path,
    session_file: &std::path::Path,
    port: u16,
) -> ChildGuard {
    spawn_native_daemon_with_args(download_dir, session_file, port, &[])
}

fn spawn_native_daemon_with_args(
    download_dir: &std::path::Path,
    session_file: &std::path::Path,
    port: u16,
    extra_args: &[std::ffi::OsString],
) -> ChildGuard {
    let mut command = Command::new(cargo_bin("raria"));
    command
        .arg("daemon")
        .arg("--download-dir")
        .arg(download_dir)
        .arg("--api-port")
        .arg(port.to_string())
        .arg("--session-path")
        .arg(session_file);
    for arg in extra_args {
        command.arg(arg);
    }
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn daemon");
    ChildGuard { child }
}

async fn wait_for_child_exit_after_forced_stop(child: &mut ChildGuard) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match child.child.try_wait() {
            Ok(Some(_status)) => return,
            Ok(None) => {
                assert!(
                    Instant::now() < deadline,
                    "daemon did not exit after forced stop"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("failed waiting for daemon exit: {error}"),
        }
    }
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

async fn wait_for_task_progress_at_least(
    port: u16,
    task_id: &str,
    min_completed_bytes: u64,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let client = reqwest::Client::new();
    loop {
        let task: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
            .send()
            .await
            .expect("task detail request")
            .json()
            .await
            .expect("task detail json");
        let completed = task["completedBytes"].as_u64().unwrap_or(0);
        if task["lifecycle"] == "running" && completed >= min_completed_bytes {
            return task;
        }

        assert!(
            Instant::now() < deadline,
            "task never accumulated required partial progress: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_task_lifecycle(
    port: u16,
    task_id: &str,
    expected_lifecycle: &str,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let client = reqwest::Client::new();
    loop {
        let task: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
            .send()
            .await
            .expect("task detail request")
            .json()
            .await
            .expect("task detail json");
        if task["lifecycle"] == expected_lifecycle {
            return task;
        }

        assert!(
            Instant::now() < deadline,
            "task never reached lifecycle {expected_lifecycle}: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_task_not_lifecycle(
    port: u16,
    task_id: &str,
    excluded_lifecycle: &str,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let client = reqwest::Client::new();
    loop {
        let task: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
            .send()
            .await
            .expect("task detail request")
            .json()
            .await
            .expect("task detail json");
        if task["lifecycle"] != excluded_lifecycle {
            return task;
        }

        assert!(
            Instant::now() < deadline,
            "task stayed in excluded lifecycle {excluded_lifecycle}: {task}"
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
    seed_addr: SocketAddr,
    _seed_root: tempfile::TempDir,
    _seed_session: Option<std::sync::Arc<RqbitSession>>,
}

struct UdpTrackerFixture {
    announce_url: String,
    announce_count: Arc<AtomicUsize>,
}

async fn spawn_udp_tracker(peer_addr: SocketAddr) -> UdpTrackerFixture {
    let socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind UDP tracker");
    let tracker_addr = socket.local_addr().expect("UDP tracker addr");
    let announce_count = Arc::new(AtomicUsize::new(0));
    let announce_count_for_task = Arc::clone(&announce_count);

    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            let Ok((len, remote)) = socket.recv_from(&mut buf).await else {
                break;
            };
            if len < 16 {
                continue;
            }

            let action = u32::from_be_bytes(buf[8..12].try_into().expect("action bytes"));
            let transaction_id = &buf[12..16];
            match action {
                0 => {
                    let mut response = Vec::with_capacity(16);
                    response.extend_from_slice(&0u32.to_be_bytes());
                    response.extend_from_slice(transaction_id);
                    response.extend_from_slice(&0x1122_3344_5566_7788u64.to_be_bytes());
                    let _ = socket.send_to(&response, remote).await;
                }
                1 => {
                    announce_count_for_task.fetch_add(1, Ordering::SeqCst);
                    let mut response = Vec::with_capacity(26);
                    response.extend_from_slice(&1u32.to_be_bytes());
                    response.extend_from_slice(transaction_id);
                    response.extend_from_slice(&60u32.to_be_bytes());
                    response.extend_from_slice(&0u32.to_be_bytes());
                    response.extend_from_slice(&1u32.to_be_bytes());
                    match peer_addr {
                        SocketAddr::V4(addr) => {
                            response.extend_from_slice(&addr.ip().octets());
                            response.extend_from_slice(&addr.port().to_be_bytes());
                        }
                        SocketAddr::V6(_) => continue,
                    }
                    let _ = socket.send_to(&response, remote).await;
                }
                _ => {}
            }
        }
    });

    UdpTrackerFixture {
        announce_url: format!("udp://{tracker_addr}/announce"),
        announce_count,
    }
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
        seed_addr: SocketAddr::from(([127, 0, 0, 1], peer_port)),
        _seed_root: seed_root,
        _seed_session: Some(session),
    }
}

struct BtWebSeedFixture {
    torrent_b64: String,
    torrent_file: tempfile::NamedTempFile,
    web_seed_url: String,
    payload_len: usize,
    server: MockServer,
}

struct RangeResponder {
    data: Arc<Vec<u8>>,
}

impl RangeResponder {
    fn new(data: Arc<Vec<u8>>) -> Self {
        Self { data }
    }
}

fn parse_range_header(header: &str, total_len: usize) -> Option<(usize, usize)> {
    let value = header.trim().strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.parse::<usize>().ok()?;
    let end = if end.is_empty() {
        total_len.checked_sub(1)?
    } else {
        end.parse::<usize>().ok()?
    };
    (start <= end && end < total_len).then_some((start, end))
}

impl Respond for RangeResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let total_len = self.data.len();
        if request.method.as_str() == "HEAD" {
            return ResponseTemplate::new(200)
                .insert_header("accept-ranges", "bytes")
                .insert_header("content-length", total_len.to_string());
        }

        let Some((start, end)) = request
            .headers
            .get("range")
            .and_then(|value| value.to_str().ok())
            .and_then(|header| parse_range_header(header, total_len))
        else {
            return ResponseTemplate::new(200)
                .insert_header("accept-ranges", "bytes")
                .insert_header("content-length", total_len.to_string())
                .set_body_bytes(self.data.as_ref().clone());
        };

        ResponseTemplate::new(206)
            .insert_header("accept-ranges", "bytes")
            .insert_header("content-range", format!("bytes {start}-{end}/{total_len}"))
            .set_body_bytes(self.data[start..=end].to_vec())
    }
}

async fn spawn_bt_web_seed_fixture(payload: Vec<u8>) -> BtWebSeedFixture {
    let payload = Arc::new(payload);
    let source_root = tempdir().expect("webseed source tempdir");
    let source_file = source_root.path().join("seed.bin");
    std::fs::write(&source_file, payload.as_ref()).expect("write webseed source payload");

    let torrent = create_torrent(
        &source_file,
        CreateTorrentOptions {
            piece_length: Some(1024),
            ..Default::default()
        },
    )
    .await
    .expect("create webseed torrent");

    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/seed.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&payload)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/seed.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&payload)))
        .mount(&server)
        .await;

    BtWebSeedFixture {
        torrent_b64: base64::engine::general_purpose::STANDARD
            .encode(torrent.as_bytes().expect("torrent bytes")),
        torrent_file: {
            let mut file = tempfile::Builder::new()
                .suffix(".torrent")
                .tempfile()
                .expect("torrent tempfile");
            std::io::Write::write_all(&mut file, &torrent.as_bytes().expect("torrent bytes"))
                .expect("write torrent file");
            file
        },
        web_seed_url: format!("{}/seed.bin", server.uri()),
        payload_len: payload.len(),
        server,
    }
}

async fn spawn_bt_multi_file_web_seed_fixture() -> BtWebSeedFixture {
    let source_root = tempdir().expect("webseed source tempdir");
    let payload_dir = source_root.path().join("payload");
    std::fs::create_dir(&payload_dir).expect("create payload dir");
    let first_payload = vec![b'a'; 1024];
    let second_payload = vec![b'b'; 1024];
    std::fs::write(payload_dir.join("file-a.bin"), &first_payload).expect("write first payload");
    std::fs::write(payload_dir.join("file-b.bin"), &second_payload).expect("write second payload");

    let torrent = create_torrent(
        &payload_dir,
        CreateTorrentOptions {
            name: Some("payload"),
            piece_length: Some(1024),
        },
    )
    .await
    .expect("create webseed torrent");

    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/payload/file-a.bin"))
        .respond_with(RangeResponder::new(Arc::new(first_payload.clone())))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/payload/file-a.bin"))
        .respond_with(RangeResponder::new(Arc::new(first_payload)))
        .mount(&server)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/payload/file-b.bin"))
        .respond_with(RangeResponder::new(Arc::new(second_payload.clone())))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/payload/file-b.bin"))
        .respond_with(RangeResponder::new(Arc::new(second_payload)))
        .mount(&server)
        .await;

    BtWebSeedFixture {
        torrent_b64: base64::engine::general_purpose::STANDARD
            .encode(torrent.as_bytes().expect("torrent bytes")),
        torrent_file: {
            let mut file = tempfile::Builder::new()
                .suffix(".torrent")
                .tempfile()
                .expect("torrent tempfile");
            std::io::Write::write_all(&mut file, &torrent.as_bytes().expect("torrent bytes"))
                .expect("write torrent file");
            file
        },
        web_seed_url: server.uri(),
        payload_len: 2048,
        server,
    }
}

async fn spawn_bt_multi_file_seed_fixture() -> BtSeedFixture {
    let seed_root = tempdir().expect("seed tempdir");
    let payload_dir = seed_root.path().join("payload");
    std::fs::create_dir(&payload_dir).expect("create payload dir");
    std::fs::write(payload_dir.join("file-a.bin"), vec![b'a'; 1024])
        .expect("write first seed payload");
    std::fs::write(payload_dir.join("file-b.bin"), vec![b'b'; 1024])
        .expect("write second seed payload");

    let torrent = create_torrent(
        &payload_dir,
        CreateTorrentOptions {
            name: Some("payload"),
            piece_length: Some(1024),
        },
    )
    .await
    .expect("create torrent");
    let tracker = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/announce"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"d8:intervali60e5:peers0:e"))
        .mount(&tracker)
        .await;

    BtSeedFixture {
        tracker_url: format!("{}/announce", tracker.uri()),
        torrent_b64: base64::engine::general_purpose::STANDARD
            .encode(torrent.as_bytes().expect("torrent bytes")),
        tracker,
        seed_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        _seed_root: seed_root,
        _seed_session: None,
    }
}

#[tokio::test]
async fn daemon_exposes_native_api_endpoints() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-api.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let ws_url = format!("ws://127.0.0.1:{port}/api/v1/events");
    let (mut events, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("native event stream connect");

    let body: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/api/v1/health"))
        .await
        .expect("health request")
        .json()
        .await
        .expect("health json");

    assert_eq!(body["status"], "ok");
    assert_eq!(body["apiVersion"], 1);
    assert!(body.get("jsonrpc").is_none());

    let tasks: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .await
        .expect("tasks request")
        .json()
        .await
        .expect("tasks json");

    assert!(tasks["tasks"].as_array().expect("tasks array").is_empty());
    assert!(tasks.get("jsonrpc").is_none());

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": ["https://example.com/file.iso"],
            "downloadDir": temp.path(),
            "filename": "file.iso",
            "segments": 2
        }))
        .send()
        .await
        .expect("create task request")
        .json()
        .await
        .expect("create task json");

    let task_id = created["taskId"].as_str().expect("task id");
    assert!(task_id.starts_with("task_"));
    assert_eq!(task_id.len(), "task_".len() + 32);
    assert!(
        matches!(created["lifecycle"].as_str(), Some("queued" | "running")),
        "created task should be queued or running, got {created}"
    );
    assert!(created.get("gid").is_none());

    let paused: serde_json::Value = client
        .post(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/pause"
        ))
        .send()
        .await
        .expect("pause task request")
        .json()
        .await
        .expect("pause task json");
    assert_eq!(paused["lifecycle"], "paused");

    let paused_event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = events
                .next()
                .await
                .expect("native event stream ended")
                .expect("native event frame");
            let json: serde_json::Value =
                serde_json::from_str(frame.to_text().expect("event text")).expect("event json");
            if json["type"] == "task.paused" {
                break json;
            }
        }
    })
    .await
    .expect("timed out waiting for native pause event");
    assert_eq!(paused_event["taskId"], task_id);
    assert!(paused_event.get("jsonrpc").is_none());
    assert!(paused_event.get("method").is_none());

    let resumed: serde_json::Value = client
        .post(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/resume"
        ))
        .send()
        .await
        .expect("resume task request")
        .json()
        .await
        .expect("resume task json");
    assert!(
        matches!(resumed["lifecycle"].as_str(), Some("queued" | "running")),
        "resumed task should be queued or running, got {resumed}"
    );

    let removed: serde_json::Value = client
        .delete(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
        .send()
        .await
        .expect("remove task request")
        .json()
        .await
        .expect("remove task json");
    assert_eq!(removed["lifecycle"], "removed");

    let saved: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/session/save"))
        .send()
        .await
        .expect("save session request")
        .json()
        .await
        .expect("save session json");
    assert_eq!(saved["status"], "saved");
    assert_eq!(saved["sessionPath"].as_str(), session_file.to_str());
    assert!(session_file.is_file());
}

#[tokio::test]
async fn daemon_native_api_shutdown_stops_daemon_without_json_rpc() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-shutdown.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{port}/api/v1/daemon/shutdown"))
        .send()
        .await
        .expect("native shutdown request");
    assert!(
        response.status().is_success(),
        "native shutdown should return success, got {}",
        response.status()
    );
    let response: serde_json::Value = response.json().await.expect("native shutdown json");

    assert_eq!(response["status"], "shuttingDown");
    assert!(response.get("jsonrpc").is_none());
    assert!(response.get("result").is_none());

    wait_for_child_exit_after_native_shutdown(&mut child).await;
}

#[tokio::test]
async fn daemon_stop_after_exits_through_native_timer() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-stop-after.session.redb");
    let port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--stop-after"),
        std::ffi::OsString::from("1"),
    ];
    let mut child = spawn_native_daemon_with_args(temp.path(), &session_file, port, &extra_args);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    wait_for_child_exit_after_native_shutdown(&mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_stop_when_parent_exits_uses_native_pid_policy() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-parent-stop.session.redb");
    let port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--stop-when-parent-exits"),
        std::ffi::OsString::from("999999"),
    ];
    let mut child = spawn_native_daemon_with_args(temp.path(), &session_file, port, &extra_args);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    wait_for_child_exit_after_native_shutdown(&mut child).await;
}

#[tokio::test]
async fn daemon_does_not_expose_legacy_jsonrpc_endpoint() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-only.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{port}/jsonrpc"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "legacy",
            "method": "aria2.getVersion",
            "params": [],
        }))
        .send()
        .await
        .expect("legacy JSON-RPC probe");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

    let shutdown = client
        .post(format!("http://127.0.0.1:{port}/api/v1/daemon/shutdown"))
        .send()
        .await
        .expect("native shutdown request");
    assert!(shutdown.status().is_success());
    wait_for_child_exit_after_native_shutdown(&mut child).await;
}

#[tokio::test]
async fn daemon_native_task_create_applies_request_headers_to_downloads() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/header.bin"))
        .and(wiremock::matchers::header("x-native-header", "from-native"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/header.bin"))
        .and(wiremock::matchers::header("x-native-header", "from-native"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"done"))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-header.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/header.bin", server.uri())],
            "downloadDir": temp.path(),
            "filename": "header.bin",
            "segments": 1,
            "headers": {
                "X-Native-Header": "from-native"
            }
        }))
        .send()
        .await
        .expect("create native header task")
        .json()
        .await
        .expect("native header task json");
    let task_id = created["taskId"].as_str().expect("task id");

    wait_for_task_lifecycle(port, task_id, "completed").await;
    assert_eq!(
        std::fs::read(temp.path().join("header.bin")).expect("read downloaded file"),
        b"done"
    );
    assert!(created.get("gid").is_none());
}

#[tokio::test]
async fn daemon_native_task_create_applies_basic_auth_to_downloads() {
    let server = MockServer::start().await;
    let auth_value = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(b"native-user:native-pass")
    );

    Mock::given(method("HEAD"))
        .and(path("/auth.bin"))
        .and(wiremock::matchers::header(
            "authorization",
            auth_value.as_str(),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/auth.bin"))
        .and(wiremock::matchers::header(
            "authorization",
            auth_value.as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"auth"))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-auth.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/auth.bin", server.uri())],
            "downloadDir": temp.path(),
            "filename": "auth.bin",
            "segments": 1,
            "auth": {
                "username": "native-user",
                "password": "native-pass"
            }
        }))
        .send()
        .await
        .expect("create native auth task")
        .json()
        .await
        .expect("native auth task json");
    let task_id = created["taskId"].as_str().expect("task id");

    wait_for_task_lifecycle(port, task_id, "completed").await;
    assert_eq!(
        std::fs::read(temp.path().join("auth.bin")).expect("read downloaded file"),
        b"auth"
    );
    assert!(created.get("gid").is_none());
}

#[tokio::test]
async fn daemon_native_task_reports_effective_active_connections() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/connections.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1048576")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/connections.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'c'; 1024 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-connections.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/connections.bin", server.uri())],
            "downloadDir": temp.path(),
            "filename": "connections.bin",
            "segments": 4
        }))
        .send()
        .await
        .expect("create native connection task")
        .json()
        .await
        .expect("native connection task json");
    let task_id = created["taskId"].as_str().expect("task id");

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let task: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
            .send()
            .await
            .expect("task detail request")
            .json()
            .await
            .expect("task detail json");
        if task["lifecycle"] == "running" && task["activeConnections"] == 4 {
            assert!(task.get("connections").is_none());
            assert!(task.get("gid").is_none());
            break;
        }
        if task["lifecycle"] == "completed" {
            panic!("task completed before exposing active native connections: {task}");
        }

        assert!(
            Instant::now() < deadline,
            "native task never exposed activeConnections=4: {task}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn daemon_native_task_stops_after_file_not_found_budget() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/missing.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1024")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/missing.bin"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-missing.session.redb");
    let port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--max-not-found"),
        std::ffi::OsString::from("1"),
        std::ffi::OsString::from("--retry-attempts"),
        std::ffi::OsString::from("10"),
    ];
    let mut child = spawn_native_daemon_with_args(temp.path(), &session_file, port, &extra_args);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/missing.bin", server.uri())],
            "downloadDir": temp.path(),
            "filename": "missing.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create native missing task")
        .json()
        .await
        .expect("native missing task json");
    let task_id = created["taskId"].as_str().expect("task id");

    let failed = wait_for_task_lifecycle(port, task_id, "failed").await;
    assert_eq!(failed["taskId"], task_id);
    assert!(failed.get("gid").is_none());

    let requests = server.received_requests().await.expect("received requests");
    let get_count = requests
        .iter()
        .filter(|request| request.method.as_str() == "GET" && request.url.path() == "/missing.bin")
        .count();
    assert_eq!(get_count, 1);
}

#[tokio::test]
async fn daemon_native_log_file_records_redacted_download_context() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/secret.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/secret.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"pass"))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-log-redaction.session.redb");
    let log_path = temp.path().join("native.log");
    let port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--log"),
        log_path.as_os_str().to_os_string(),
    ];
    let mut child = spawn_native_daemon_with_args(temp.path(), &session_file, port, &extra_args);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let credentialed = format!(
        "http://alice:supersecret@127.0.0.1:{}/secret.bin?token=abc",
        server.address().port()
    );
    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [credentialed],
            "downloadDir": temp.path(),
            "filename": "secret.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create native redaction task")
        .json()
        .await
        .expect("native redaction task json");
    let task_id = created["taskId"].as_str().expect("task id");
    wait_for_task_lifecycle(port, task_id, "completed").await;

    let shutdown = client
        .post(format!("http://127.0.0.1:{port}/api/v1/daemon/shutdown"))
        .send()
        .await
        .expect("native shutdown request");
    assert!(shutdown.status().is_success());
    wait_for_child_exit_after_native_shutdown(&mut child).await;

    let log = std::fs::read_to_string(&log_path).expect("read native log file");
    let entries = log
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSON log line"))
        .collect::<Vec<_>>();
    assert!(
        entries
            .iter()
            .any(|entry| entry["message"] == "daemon: starting download"
                && entry["fields"]["task_id"] == task_id
                && entry["fields"]["uri"]
                    .as_str()
                    .is_some_and(|uri| uri.contains("/secret.bin"))),
        "native structured log should retain task and path context"
    );
    assert!(!log.contains("supersecret"));
    assert!(!log.contains("token=abc"));
    assert!(entries.iter().any(|entry| entry.get("level").is_some()));
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_flag_detaches_process_and_keeps_native_api_alive() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-daemonize.session.redb");
    let port = allocate_port();

    let mut child = Command::new(cargo_bin("raria"))
        .arg("daemon")
        .arg("--download-dir")
        .arg(temp.path())
        .arg("--api-port")
        .arg(port.to_string())
        .arg("--session-path")
        .arg(&session_file)
        .arg("--detach")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemonize request");

    let exit_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(
                    status.success(),
                    "daemonizing parent exited unsuccessfully: {status}"
                );
                break;
            }
            Ok(None) => {
                assert!(
                    Instant::now() < exit_deadline,
                    "daemonizing parent did not exit promptly"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("failed waiting for daemonizing parent: {error}"),
        }
    }

    let client = reqwest::Client::new();
    let ready_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{port}/api/v1/health"))
            .send()
            .await
        {
            if resp.status().is_success() {
                break;
            }
        }

        assert!(
            Instant::now() < ready_deadline,
            "background daemon never became native API ready"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let shutdown = client
        .post(format!("http://127.0.0.1:{port}/api/v1/daemon/shutdown"))
        .send()
        .await
        .expect("native shutdown request");
    assert!(shutdown.status().is_success());
}

#[tokio::test]
async fn daemon_native_transfer_policy_mutates_runtime_state() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-transfer-policy.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let patched: serde_json::Value = client
        .patch(format!("http://127.0.0.1:{port}/api/v1/transfer"))
        .json(&serde_json::json!({
            "downloadBytesPerSecondLimit": 131072,
            "uploadBytesPerSecondLimit": 65536,
            "maxActiveTasks": 3
        }))
        .send()
        .await
        .expect("global transfer patch request")
        .json()
        .await
        .expect("global transfer patch json");

    assert_eq!(patched["downloadBytesPerSecondLimit"], 131072);
    assert_eq!(patched["uploadBytesPerSecondLimit"], 65536);
    assert_eq!(patched["maxActiveTasks"], 3);
    assert!(patched.get("max-overall-download-limit").is_none());
    assert!(patched.get("jsonrpc").is_none());

    let readback: serde_json::Value = client
        .get(format!("http://127.0.0.1:{port}/api/v1/transfer"))
        .send()
        .await
        .expect("global transfer read request")
        .json()
        .await
        .expect("global transfer read json");
    assert_eq!(readback["downloadBytesPerSecondLimit"], 131072);
    assert_eq!(readback["uploadBytesPerSecondLimit"], 65536);
    assert_eq!(readback["maxActiveTasks"], 3);
}

#[tokio::test]
async fn daemon_native_task_mutation_routes_update_waiting_tasks() {
    let hold = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/hold.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1048576")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&hold)
        .await;
    Mock::given(method("GET"))
        .and(path("/hold.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(30))
                .set_body_bytes(vec![b'h'; 1024 * 1024]),
        )
        .mount(&hold)
        .await;

    let mirror_a = MockServer::start().await;
    let mirror_b = MockServer::start().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-task-mutation.session.redb");
    let port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--max-concurrent"),
        std::ffi::OsString::from("1"),
    ];
    let mut child = spawn_native_daemon_with_args(temp.path(), &session_file, port, &extra_args);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let holding: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/hold.bin", hold.uri())],
            "downloadDir": temp.path(),
            "filename": "hold.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create holding task")
        .json()
        .await
        .expect("holding task json");
    let holding_task_id = holding["taskId"].as_str().expect("holding task id");
    wait_for_task_lifecycle(port, holding_task_id, "running").await;

    let first_waiting: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": ["https://old.example/first.bin"],
            "downloadDir": temp.path(),
            "filename": "first.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create first waiting task")
        .json()
        .await
        .expect("first waiting task json");
    let second_waiting: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": ["https://old.example/second.bin"],
            "downloadDir": temp.path(),
            "filename": "second.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create second waiting task")
        .json()
        .await
        .expect("second waiting task json");
    let first_task_id = first_waiting["taskId"].as_str().expect("first task id");
    let second_task_id = second_waiting["taskId"].as_str().expect("second task id");

    let task_transfer: serde_json::Value = client
        .patch(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{first_task_id}/transfer"
        ))
        .json(&serde_json::json!({
            "downloadBytesPerSecondLimit": 98304,
            "uploadBytesPerSecondLimit": 49152,
            "segments": 3
        }))
        .send()
        .await
        .expect("task transfer patch request")
        .json()
        .await
        .expect("task transfer patch json");
    assert_eq!(task_transfer["downloadBytesPerSecondLimit"], 98304);
    assert_eq!(task_transfer["uploadBytesPerSecondLimit"], 49152);
    assert_eq!(task_transfer["segments"], 3);
    assert!(task_transfer.get("max-download-limit").is_none());

    let sources: serde_json::Value = client
        .patch(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{first_task_id}/sources"
        ))
        .json(&serde_json::json!({
            "sources": [
                format!("{}/first.bin", mirror_a.uri()),
                format!("{}/first.bin", mirror_b.uri())
            ]
        }))
        .send()
        .await
        .expect("task sources patch request")
        .json()
        .await
        .expect("task sources patch json");
    assert_eq!(sources["sources"][0]["protocol"], "http");
    assert_eq!(
        sources["sources"][0]["uri"],
        format!("{}/first.bin", mirror_a.uri())
    );
    assert_eq!(
        sources["sources"][1]["uri"],
        format!("{}/first.bin", mirror_b.uri())
    );
    assert!(sources.get("gid").is_none());

    let queue: serde_json::Value = client
        .patch(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{second_task_id}/queue"
        ))
        .json(&serde_json::json!({
            "position": 0
        }))
        .send()
        .await
        .expect("queue patch request")
        .json()
        .await
        .expect("queue patch json");
    assert_eq!(queue["taskId"], second_task_id);
    assert_eq!(queue["position"], 0);
    assert!(queue.get("how").is_none());

    let first_queue: serde_json::Value = client
        .get(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{first_task_id}/queue"
        ))
        .send()
        .await
        .expect("first queue read request")
        .json()
        .await
        .expect("first queue read json");
    assert_eq!(first_queue["position"], 1);

    let first_transfer: serde_json::Value = client
        .get(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{first_task_id}/transfer"
        ))
        .send()
        .await
        .expect("first transfer read request")
        .json()
        .await
        .expect("first transfer read json");
    assert_eq!(first_transfer["downloadBytesPerSecondLimit"], 98304);
    assert_eq!(first_transfer["uploadBytesPerSecondLimit"], 49152);
    assert_eq!(first_transfer["segments"], 3);
}

#[tokio::test]
async fn daemon_native_api_creates_and_completes_metalink_tasks() {
    let first_payload = Arc::new(vec![b'a'; 16 * 1024]);
    let second_payload = Arc::new(vec![b'b'; 8 * 1024]);
    let mirror = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/alpha.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&first_payload)))
        .mount(&mirror)
        .await;
    Mock::given(method("GET"))
        .and(path("/alpha.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&first_payload)))
        .mount(&mirror)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/beta.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&second_payload)))
        .mount(&mirror)
        .await;
    Mock::given(method("GET"))
        .and(path("/beta.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&second_payload)))
        .mount(&mirror)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-metalink.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let metalink_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="alpha.bin">
    <size>{}</size>
    <url priority="1">{}/alpha.bin</url>
  </file>
  <file name="beta.bin">
    <size>{}</size>
    <url priority="1">{}/beta.bin</url>
  </file>
</metalink>"#,
        first_payload.len(),
        mirror.uri(),
        second_payload.len(),
        mirror.uri()
    );

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "downloadDir": temp.path(),
            "metalink": {
                "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
            }
        }))
        .send()
        .await
        .expect("create metalink request")
        .json()
        .await
        .expect("create metalink json");
    let tasks = created["tasks"].as_array().expect("created tasks");
    assert_eq!(tasks.len(), 2);
    assert!(created.get("gid").is_none());

    for task in tasks {
        let task_id = task["taskId"].as_str().expect("task id");
        let completed = wait_for_task_lifecycle(port, task_id, "completed").await;
        assert_eq!(completed["taskId"], task_id);
        assert_eq!(completed["completedBytes"], completed["totalBytes"]);
    }

    assert_eq!(
        std::fs::read(temp.path().join("alpha.bin")).expect("alpha output"),
        first_payload.as_ref().as_slice()
    );
    assert_eq!(
        std::fs::read(temp.path().join("beta.bin")).expect("beta output"),
        second_payload.as_ref().as_slice()
    );
}

#[tokio::test]
async fn daemon_native_api_enforces_metalink_checksum_failure() {
    let payload = Arc::new(vec![b'x'; 4096]);
    let mirror = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/bad-checksum.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&payload)))
        .mount(&mirror)
        .await;
    Mock::given(method("GET"))
        .and(path("/bad-checksum.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&payload)))
        .mount(&mirror)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-metalink-checksum.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let metalink_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="bad-checksum.bin">
    <size>{}</size>
    <hash type="sha-256">0000000000000000000000000000000000000000000000000000000000000000</hash>
    <url priority="1">{}/bad-checksum.bin</url>
  </file>
</metalink>"#,
        payload.len(),
        mirror.uri()
    );

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "downloadDir": temp.path(),
            "metalink": {
                "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
            }
        }))
        .send()
        .await
        .expect("create metalink request")
        .json()
        .await
        .expect("create metalink json");
    let tasks = created["tasks"].as_array().expect("created tasks");
    assert_eq!(tasks.len(), 1);
    let task_id = tasks[0]["taskId"].as_str().expect("task id");

    let failed = wait_for_task_lifecycle(port, task_id, "failed").await;
    assert_eq!(failed["taskId"], task_id);
    assert_ne!(failed["lifecycle"], "completed");
    assert!(failed.get("gid").is_none());

    let source_error = failed["sources"][0]["health"]["lastError"]
        .as_str()
        .expect("source error");
    assert!(
        source_error.contains("checksum verification failed"),
        "source error should report checksum verification failure, got {source_error}"
    );
}

#[tokio::test]
async fn daemon_native_api_enforces_metalink_piece_checksum_failure() {
    let payload = Arc::new(b"abcdefgh".to_vec());
    let mirror = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/piece.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&payload)))
        .mount(&mirror)
        .await;
    Mock::given(method("GET"))
        .and(path("/piece.bin"))
        .respond_with(RangeResponder::new(Arc::clone(&payload)))
        .mount(&mirror)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp
        .path()
        .join("native-metalink-piece-checksum.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let metalink_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="piece.bin">
    <size>8</size>
    <pieces type="sha-256" length="4">
      <hash>{}</hash>
      <hash>{}</hash>
    </pieces>
    <url priority="1">{}/piece.bin</url>
  </file>
</metalink>"#,
        "00".repeat(32),
        "e5e088a0b66163a0a26a5e053d2a4496dc16ab6e0e3dd1adf2d16aa84a078c9d",
        mirror.uri()
    );

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "downloadDir": temp.path(),
            "metalink": {
                "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
            }
        }))
        .send()
        .await
        .expect("create metalink request")
        .json()
        .await
        .expect("create metalink json");
    let task_id = created["tasks"][0]["taskId"].as_str().expect("task id");

    let failed = wait_for_task_lifecycle(port, task_id, "failed").await;
    assert_eq!(failed["taskId"], task_id);
    let source_error = failed["sources"][0]["health"]["lastError"]
        .as_str()
        .expect("source error");
    assert!(
        source_error.contains("piece checksum"),
        "source error should report piece checksum verification failure, got {source_error}"
    );
    assert!(
        !temp.path().join("piece.bin").exists(),
        "piece checksum mismatch should remove the invalid output file"
    );
}

#[tokio::test]
async fn daemon_native_api_metalink_fails_over_after_mirror_transfer_error() {
    let primary = MockServer::start().await;
    let fallback = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/metalink-mirror.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&primary)
        .await;
    Mock::given(method("GET"))
        .and(path("/metalink-mirror.bin"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&primary)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/metalink-mirror.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&fallback)
        .await;
    Mock::given(method("GET"))
        .and(path("/metalink-mirror.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"pass"))
        .mount(&fallback)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-metalink-mirror.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let metalink_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="metalink-mirror.bin">
    <size>4</size>
    <url priority="1">{}/metalink-mirror.bin</url>
    <url priority="2">{}/metalink-mirror.bin</url>
  </file>
</metalink>"#,
        primary.uri(),
        fallback.uri()
    );

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "downloadDir": temp.path(),
            "metalink": {
                "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
            }
        }))
        .send()
        .await
        .expect("create metalink request")
        .json()
        .await
        .expect("create metalink json");
    let task_id = created["tasks"][0]["taskId"].as_str().expect("task id");

    let completed = wait_for_task_lifecycle(port, task_id, "completed").await;
    assert_eq!(completed["taskId"], task_id);
    assert!(completed.get("gid").is_none());
    assert_eq!(
        std::fs::read(temp.path().join("metalink-mirror.bin")).expect("metalink output"),
        b"pass"
    );
    assert!(
        primary.received_requests().await.unwrap().len() >= 2,
        "expected primary mirror to be attempted first"
    );
    assert!(
        fallback.received_requests().await.unwrap().len() >= 2,
        "expected fallback mirror to be attempted after primary failure"
    );
}

#[tokio::test]
async fn daemon_native_api_metalink_fails_over_after_checksum_mismatch() {
    let primary = MockServer::start().await;
    let fallback = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/mirror.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&primary)
        .await;
    Mock::given(method("GET"))
        .and(path("/mirror.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"bad!"))
        .mount(&primary)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/mirror.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&fallback)
        .await;
    Mock::given(method("GET"))
        .and(path("/mirror.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"pass"))
        .mount(&fallback)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp
        .path()
        .join("native-metalink-checksum-failover.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);
    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let metalink_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="mirror.bin">
    <size>4</size>
    <hash type="sha-256">d74ff0ee8da3b9806b18c877dbf29bbde50b5bd8e4dad7a3a725000feb82e8f1</hash>
    <url priority="1">{}/mirror.bin</url>
    <url priority="2">{}/mirror.bin</url>
  </file>
</metalink>"#,
        primary.uri(),
        fallback.uri()
    );

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "downloadDir": temp.path(),
            "metalink": {
                "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
            }
        }))
        .send()
        .await
        .expect("create metalink request")
        .json()
        .await
        .expect("create metalink json");
    let task_id = created["tasks"][0]["taskId"].as_str().expect("task id");

    let completed = wait_for_task_lifecycle(port, task_id, "completed").await;
    assert_eq!(completed["taskId"], task_id);
    assert_eq!(
        std::fs::read(temp.path().join("mirror.bin")).expect("metalink output"),
        b"pass"
    );
    assert!(
        primary.received_requests().await.unwrap().len() >= 2,
        "expected checksum-failing primary mirror to be attempted first"
    );
    assert!(
        fallback.received_requests().await.unwrap().len() >= 2,
        "expected fallback mirror to be attempted after checksum mismatch"
    );
}

#[tokio::test]
async fn daemon_native_api_exposes_live_bt_metadata_peers_and_trackers() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_seed_fixture_with_payload(
        (0..(8 * 1024 * 1024))
            .map(|idx| (idx % 251) as u8)
            .collect(),
    )
    .await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let ws_url = format!("ws://127.0.0.1:{port}/api/v1/events");
    let (mut events, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("native event stream connect");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("torrent:base64:{}", fixture.torrent_b64)],
            "downloadDir": temp.path(),
            "filename": "seed.bin",
            "bt": {
                "trackerUris": [fixture.tracker_url]
            }
        }))
        .send()
        .await
        .expect("native BT create request")
        .json()
        .await
        .expect("native BT create json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();
    assert_eq!(created["sources"][0]["protocol"], "torrent");
    assert!(created.get("gid").is_none());
    assert!(created.get("bt-tracker").is_none());

    let metadata_event = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let frame = events
                .next()
                .await
                .expect("native event stream ended")
                .expect("native event frame");
            let json: serde_json::Value =
                serde_json::from_str(frame.to_text().expect("event text")).expect("event json");
            if json["type"] == "task.bt.metadata.resolved" && json["taskId"] == task_id {
                break json;
            }
        }
    })
    .await
    .expect("timed out waiting for native BT metadata event");
    assert_eq!(metadata_event["data"]["kind"], "btMetadata");
    assert_eq!(metadata_event["data"]["name"], "seed.bin");
    assert_eq!(metadata_event["data"]["totalBytes"], 8 * 1024 * 1024);
    assert!(metadata_event.get("jsonrpc").is_none());
    assert!(metadata_event.get("method").is_none());

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let task: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
            .send()
            .await
            .expect("native BT task detail request")
            .json()
            .await
            .expect("native BT task detail json");
        let trackers: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/trackers"
            ))
            .send()
            .await
            .expect("native BT trackers request")
            .json()
            .await
            .expect("native BT trackers json");
        let peers: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/peers"
            ))
            .send()
            .await
            .expect("native BT peers request")
            .json()
            .await
            .expect("native BT peers json");
        let tracker_requests = fixture.tracker.received_requests().await;

        let tracker_ready = trackers["trackers"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|tracker| tracker["uri"] == fixture.tracker_url)
        });
        let peer_ready = peers["peers"]
            .as_array()
            .is_some_and(|items| !items.is_empty());
        if tracker_ready && peer_ready {
            assert!(
                tracker_requests
                    .as_ref()
                    .is_some_and(|requests| !requests.is_empty()),
                "native BT task should announce to the configured tracker: {tracker_requests:#?}"
            );
            let peer = peers["peers"]
                .as_array()
                .and_then(|items| items.first())
                .expect("native BT peer");
            assert!(peer["id"].as_str().expect("peer id").starts_with("peer_"));
            assert!(peer["ip"].as_str().is_some());
            assert!(peer["port"].as_u64().is_some());
            assert!(peer["downloadBytesPerSecond"].as_u64().is_some());
            assert!(peer["uploadBytesPerSecond"].as_u64().is_some());
            assert!(peer["seeder"].as_bool().is_some());
            assert!(peer.get("peerId").is_none());
            assert!(peer.get("bitfield").is_none());
            assert!(trackers["trackers"][0].get("bt-tracker").is_none());
            assert!(task["totalBytes"].as_u64().is_some());
            assert!(task.get("gid").is_none());
            break;
        }

        assert!(
            Instant::now() < deadline,
            "native BT task never exposed tracker and peer state\ntask: {task}\ntrackers: {trackers}\npeers: {peers}\ntracker_requests: {tracker_requests:#?}"
        );

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test]
async fn daemon_native_api_exposes_udp_bt_tracker_projection() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_seed_fixture_with_payload(
        (0..(2 * 1024 * 1024))
            .map(|idx| (idx % 251) as u8)
            .collect(),
    )
    .await;
    let udp_tracker = spawn_udp_tracker(fixture.seed_addr).await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt-udp-tracker.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("torrent:base64:{}", fixture.torrent_b64)],
            "downloadDir": temp.path(),
            "filename": "seed.bin",
            "bt": {
                "trackerUris": [udp_tracker.announce_url]
            }
        }))
        .send()
        .await
        .expect("native BT create request")
        .json()
        .await
        .expect("native BT create json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();
    assert!(created.get("gid").is_none());

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let task: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/api/v1/tasks/{task_id}"))
            .send()
            .await
            .expect("native BT task detail request")
            .json()
            .await
            .expect("native BT task detail json");
        let trackers: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/trackers"
            ))
            .send()
            .await
            .expect("native BT trackers request")
            .json()
            .await
            .expect("native BT trackers json");
        let peers: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/peers"
            ))
            .send()
            .await
            .expect("native BT peers request")
            .json()
            .await
            .expect("native BT peers json");

        let tracker_ready = trackers["trackers"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|tracker| tracker["uri"] == udp_tracker.announce_url)
        });
        let announced = udp_tracker.announce_count.load(Ordering::SeqCst) > 0;
        let complete =
            task["lifecycle"] == "seeding" || task["completedBytes"] == task["totalBytes"];
        if tracker_ready && announced && complete {
            assert!(trackers["trackers"][0].get("bt-tracker").is_none());
            assert!(task.get("gid").is_none());
            break;
        }

        assert!(
            Instant::now() < deadline,
            "native BT task never completed through UDP tracker\ntask: {task}\ntrackers: {trackers}\npeers: {peers}\nannounces: {}",
            udp_tracker.announce_count.load(Ordering::SeqCst)
        );

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test]
async fn daemon_native_api_updates_live_bt_file_selection() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_multi_file_seed_fixture().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt-selection.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("torrent:base64:{}", fixture.torrent_b64)],
            "downloadDir": temp.path(),
            "filename": "payload",
            "bt": {
                "trackerUris": [fixture.tracker_url],
                "selectedFileIds": ["file_0"]
            }
        }))
        .send()
        .await
        .expect("native BT create request")
        .json()
        .await
        .expect("native BT create json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let files: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/files"
            ))
            .send()
            .await
            .expect("native BT files request")
            .json()
            .await
            .expect("native BT files json");
        let file_items = files["files"].as_array().cloned().unwrap_or_default();
        if file_items.len() >= 2 {
            assert_eq!(file_items[0]["id"], "file_0");
            assert_eq!(file_items[1]["id"], "file_1");
            assert_eq!(file_items[0]["selected"], true);
            assert_eq!(file_items[1]["selected"], false);
            assert!(file_items[0].get("index").is_none());
            break;
        }

        assert!(
            Instant::now() < deadline,
            "native BT task never exposed multi-file metadata: {files}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    let patched: serde_json::Value = client
        .patch(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/files"
        ))
        .json(&serde_json::json!({
            "selectedFileIds": ["file_1"]
        }))
        .send()
        .await
        .expect("native BT files patch request")
        .json()
        .await
        .expect("native BT files patch json");
    assert_eq!(patched["files"][0]["selected"], false);
    assert_eq!(patched["files"][1]["selected"], true);
    assert!(patched.get("select-file").is_none());

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let files: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/files"
            ))
            .send()
            .await
            .expect("native BT files readback request")
            .json()
            .await
            .expect("native BT files readback json");
        let file_items = files["files"].as_array().cloned().unwrap_or_default();
        if file_items.len() >= 2
            && file_items[0]["selected"] == false
            && file_items[1]["selected"] == true
        {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "native BT live file selection was not retained after runtime sync: {files}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test]
async fn daemon_native_bt_deletes_unselected_files_after_completion() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_multi_file_web_seed_fixture().await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt-delete-unselected.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("torrent:base64:{}", fixture.torrent_b64)],
            "downloadDir": temp.path(),
            "filename": "payload",
            "bt": {
                "webSeedUris": [fixture.web_seed_url],
                "selectedFileIds": ["file_1"],
                "deleteUnselectedFilesOnCompletion": true
            }
        }))
        .send()
        .await
        .expect("native BT create request")
        .json()
        .await
        .expect("native BT create json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();

    let task = wait_for_task_lifecycle(port, &task_id, "completed").await;
    assert_eq!(task["taskId"], task_id);
    assert_eq!(task["completedBytes"], 1024);

    let selected_path = temp.path().join("file-a.bin");
    let unselected_path = temp.path().join("file-b.bin");
    assert!(
        selected_path.is_file(),
        "selected BT file should remain after completion"
    );
    assert!(
        !unselected_path.exists(),
        "unselected BT file should be removed after completion"
    );
}

#[tokio::test]
async fn daemon_native_api_emits_bt_seeding_lifecycle() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_web_seed_fixture(vec![b's'; 256 * 1024]).await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt-seeding.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let ws_url = format!("ws://127.0.0.1:{port}/api/v1/events");
    let (mut events, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("native event stream connect");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("torrent:base64:{}", fixture.torrent_b64)],
            "downloadDir": temp.path(),
            "filename": "seed.bin",
            "bt": {
                "webSeedUris": [fixture.web_seed_url],
                "seeding": {
                    "targetRatio": 1000.0,
                    "stopAfterMinutes": 10,
                    "idleDownloadTimeoutSeconds": 9
                }
            }
        }))
        .send()
        .await
        .expect("native BT create request")
        .json()
        .await
        .expect("native BT create json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();

    let policy: serde_json::Value = client
        .get(format!(
            "http://127.0.0.1:{port}/api/v1/tasks/{task_id}/bt/seeding"
        ))
        .send()
        .await
        .expect("native BT seeding policy request")
        .json()
        .await
        .expect("native BT seeding policy json");
    assert_eq!(policy["targetRatio"], 1000.0);
    assert_eq!(policy["stopAfterMinutes"], 10);
    assert_eq!(policy["idleDownloadTimeoutSeconds"], 9);
    assert!(policy.get("seed-ratio").is_none());
    assert!(policy.get("bt-stop-timeout").is_none());

    let seeding_event = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let frame = events
                .next()
                .await
                .expect("native event stream ended")
                .expect("native event frame");
            let json: serde_json::Value =
                serde_json::from_str(frame.to_text().expect("event text")).expect("event json");
            if json["type"] == "task.bt.seeding.started" && json["taskId"] == task_id {
                break json;
            }
        }
    })
    .await
    .expect("timed out waiting for native BT seeding event");
    assert_eq!(seeding_event["data"]["kind"], "btSeeding");
    assert!(seeding_event["data"]["uploadedBytes"].as_u64().is_some());
    assert!(seeding_event["data"]["peerCount"].as_u64().is_some());
    assert!(seeding_event.get("jsonrpc").is_none());
    assert!(seeding_event.get("method").is_none());

    let task = wait_for_task_lifecycle(port, &task_id, "seeding").await;
    assert_eq!(task["taskId"], task_id);
    assert_eq!(task["completedBytes"], fixture.payload_len as u64);
    assert_eq!(task["totalBytes"], fixture.payload_len as u64);
    assert!(task.get("gid").is_none());

    let requests = fixture
        .server
        .received_requests()
        .await
        .expect("webseed received requests");
    assert!(
        requests
            .iter()
            .any(|request| request.method.as_str() == "GET"),
        "WebSeed should serve the BT payload before the daemon enters seeding"
    );
}

#[tokio::test]
async fn daemon_native_bt_seeding_frees_download_concurrency_for_waiting_tasks() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_web_seed_fixture(vec![b'q'; 128 * 1024]).await;
    let waiting_server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/queued.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "32768")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&waiting_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/queued.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'w'; 32768]))
        .mount(&waiting_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt-seed-only-queue.session.redb");
    let port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--max-concurrent"),
        std::ffi::OsString::from("1"),
    ];
    let mut child = spawn_native_daemon_with_args(temp.path(), &session_file, port, &extra_args);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let bt_created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("torrent:base64:{}", fixture.torrent_b64)],
            "downloadDir": temp.path(),
            "filename": "seed.bin",
            "bt": {
                "webSeedUris": [fixture.web_seed_url],
                "seeding": {
                    "targetRatio": 1000.0,
                    "stopAfterMinutes": 10
                }
            }
        }))
        .send()
        .await
        .expect("native BT create request")
        .json()
        .await
        .expect("native BT create json");
    let bt_task_id = bt_created["taskId"]
        .as_str()
        .expect("bt task id")
        .to_string();

    let waiting_created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/queued.bin", waiting_server.uri())],
            "downloadDir": temp.path(),
            "filename": "queued.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("queued task create request")
        .json()
        .await
        .expect("queued task create json");
    let waiting_task_id = waiting_created["taskId"]
        .as_str()
        .expect("waiting task id")
        .to_string();
    assert_eq!(waiting_created["lifecycle"], "queued");

    let bt_task = wait_for_task_lifecycle(port, &bt_task_id, "seeding").await;
    assert_eq!(bt_task["taskId"], bt_task_id);
    assert_eq!(bt_task["completedBytes"], fixture.payload_len as u64);

    let waiting_task = wait_for_task_not_lifecycle(port, &waiting_task_id, "queued").await;
    assert_eq!(waiting_task["taskId"], waiting_task_id);
    assert!(
        matches!(
            waiting_task["lifecycle"].as_str(),
            Some("running" | "completed")
        ),
        "waiting task should activate after the BT task becomes seed-only: {waiting_task}"
    );
    assert!(waiting_task.get("gid").is_none());
}

#[tokio::test]
async fn daemon_native_api_accepts_torrent_file_sources() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_web_seed_fixture(vec![b'f'; 128 * 1024]).await;
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt-torrent-file.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [fixture.torrent_file.path()],
            "downloadDir": temp.path(),
            "filename": "seed.bin",
            "bt": {
                "webSeedUris": [fixture.web_seed_url],
                "seeding": {
                    "targetRatio": 1000.0
                }
            }
        }))
        .send()
        .await
        .expect("native BT torrent-file create request")
        .json()
        .await
        .expect("native BT torrent-file create json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();
    assert_eq!(created["sources"][0]["protocol"], "torrent");
    assert_eq!(
        created["sources"][0]["uri"],
        fixture.torrent_file.path().to_string_lossy().as_ref()
    );
    assert!(created.get("gid").is_none());

    let task = wait_for_task_lifecycle(port, &task_id, "seeding").await;
    assert_eq!(task["taskId"], task_id);
    assert_eq!(task["completedBytes"], fixture.payload_len as u64);
    assert_eq!(task["totalBytes"], fixture.payload_len as u64);
    assert!(task.get("gid").is_none());
}

#[tokio::test]
async fn daemon_native_api_fetches_remote_torrent_metadata_sources() {
    let _guard = lock_bt_daemon_smoke_lane().await;
    let fixture = spawn_bt_web_seed_fixture(vec![b'm'; 128 * 1024]).await;
    let torrent_bytes = std::fs::read(fixture.torrent_file.path()).expect("read torrent fixture");
    let metadata = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/seed.torrent"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(torrent_bytes))
        .mount(&metadata)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-bt-remote-torrent.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let remote_torrent_uri = format!("{}/seed.torrent", metadata.uri());
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [remote_torrent_uri],
            "downloadDir": temp.path(),
            "filename": "seed.bin",
            "bt": {
                "webSeedUris": [fixture.web_seed_url],
                "seeding": {
                    "targetRatio": 1000.0
                }
            }
        }))
        .send()
        .await
        .expect("native remote torrent create request")
        .json()
        .await
        .expect("native remote torrent create json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();
    assert_eq!(created["sources"][0]["protocol"], "torrent");
    assert_eq!(created["sources"][0]["uri"], remote_torrent_uri);
    assert!(created.get("gid").is_none());

    let task = wait_for_task_lifecycle(port, &task_id, "seeding").await;
    assert_eq!(task["taskId"], task_id);
    assert_eq!(task["completedBytes"], fixture.payload_len as u64);
    assert_eq!(task["totalBytes"], fixture.payload_len as u64);
    assert!(
        metadata
            .received_requests()
            .await
            .expect("metadata requests")
            .iter()
            .any(|request| request.url.path() == "/seed.torrent")
    );
}

#[tokio::test]
async fn daemon_native_api_uses_raria_toml_bearer_auth() {
    let temp = tempdir().expect("tempdir");
    let token_file = temp.path().join("api.token");
    std::fs::write(&token_file, "secret-token\n").expect("token file");
    let config_file = temp.path().join("raria.toml");
    std::fs::write(
        &config_file,
        format!(
            r#"
[api]
auth_token_file = "{}"
"#,
            token_file.display()
        ),
    )
    .expect("config file");

    let session_file = temp.path().join("native-auth.session.redb");
    let port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--config"),
        config_file.as_os_str().to_os_string(),
    ];
    let mut child = spawn_native_daemon_with_args(temp.path(), &session_file, port, &extra_args);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let client = reqwest::Client::new();
    let unauthenticated = client
        .get(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .send()
        .await
        .expect("tasks request");
    assert_eq!(unauthenticated.status(), reqwest::StatusCode::UNAUTHORIZED);

    let authenticated = client
        .get(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .bearer_auth("secret-token")
        .send()
        .await
        .expect("authenticated tasks request");
    assert!(authenticated.status().is_success());
}

#[tokio::test]
async fn daemon_native_events_include_source_failover() {
    let fallback = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/source-failover.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&fallback)
        .await;
    Mock::given(method("GET"))
        .and(path("/source-failover.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"pass"))
        .mount(&fallback)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-source-failover.session.redb");
    let port = allocate_port();
    let mut child = spawn_native_daemon(temp.path(), &session_file, port);

    wait_for_native_api_ready(port, &mut child)
        .await
        .expect("native API ready");

    let ws_url = format!("ws://127.0.0.1:{port}/api/v1/events");
    let (mut events, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("native event stream connect");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [
                "gopher://example.invalid/source-failover.bin",
                format!("{}/source-failover.bin", fallback.uri())
            ],
            "downloadDir": temp.path(),
            "filename": "source-failover.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create task request")
        .json()
        .await
        .expect("create task json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();

    let event = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = events
                .next()
                .await
                .expect("native event stream ended")
                .expect("native event frame");
            let json: serde_json::Value =
                serde_json::from_str(frame.to_text().expect("event text")).expect("event json");
            if json["type"] == "task.source.failed" {
                break json;
            }
        }
    })
    .await
    .expect("timed out waiting for native source failure event");

    assert_eq!(event["taskId"], task_id);
    assert_eq!(
        event["data"]["uri"],
        "gopher://example.invalid/source-failover.bin"
    );
    assert_eq!(event["data"]["code"], "source_failed");
    assert!(
        event["data"]["message"]
            .as_str()
            .expect("source failure message")
            .contains("permanent error")
    );
    assert!(event.get("jsonrpc").is_none());
    assert!(event.get("method").is_none());
}

#[tokio::test]
async fn daemon_restores_saved_task_through_native_api() {
    let fallback = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/restore.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1048576")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&fallback)
        .await;
    Mock::given(method("GET"))
        .and(path("/restore.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_bytes(vec![b'x'; 1024 * 1024]),
        )
        .mount(&fallback)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-restore.session.redb");
    let first_port = allocate_port();
    let mut first = spawn_native_daemon(temp.path(), &session_file, first_port);
    wait_for_native_api_ready(first_port, &mut first)
        .await
        .expect("first daemon native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{first_port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/restore.bin", fallback.uri())],
            "downloadDir": temp.path(),
            "filename": "restore.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create task request")
        .json()
        .await
        .expect("create task json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();

    let saved: serde_json::Value = client
        .post(format!("http://127.0.0.1:{first_port}/api/v1/session/save"))
        .send()
        .await
        .expect("save session request")
        .json()
        .await
        .expect("save session json");
    assert_eq!(saved["status"], "saved");
    first.child.kill().expect("stop first daemon");
    wait_for_child_exit_after_forced_stop(&mut first).await;
    assert!(session_file.is_file());

    let second_port = allocate_port();
    let mut second = spawn_native_daemon(temp.path(), &session_file, second_port);
    wait_for_native_api_ready(second_port, &mut second)
        .await
        .expect("second daemon native API ready");

    let tasks: serde_json::Value = client
        .get(format!("http://127.0.0.1:{second_port}/api/v1/tasks"))
        .send()
        .await
        .expect("tasks request")
        .json()
        .await
        .expect("tasks json");
    let restored = tasks["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .find(|task| task["taskId"] == task_id)
        .expect("restored task");
    assert!(
        matches!(
            restored["lifecycle"].as_str(),
            Some("queued" | "running" | "completed")
        ),
        "expected restored task to be queued, running, or completed, got {restored}"
    );
    assert!(restored.get("gid").is_none());
}

#[tokio::test]
async fn daemon_resume_uses_native_segment_rows_after_restart() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/native-resume.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "8388608")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/native-resume.bin"))
        .and(wiremock::matchers::header_exists("range"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(vec![b'n'; 8 * 1024 * 1024]))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/native-resume.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_bytes(vec![b'n'; 8 * 1024 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-segment-resume.session.redb");
    let first_port = allocate_port();
    let extra_args = vec![
        std::ffi::OsString::from("--download-limit"),
        std::ffi::OsString::from("262144"),
    ];
    let mut first =
        spawn_native_daemon_with_args(temp.path(), &session_file, first_port, &extra_args);
    wait_for_native_api_ready(first_port, &mut first)
        .await
        .expect("first daemon native API ready");

    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("http://127.0.0.1:{first_port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [format!("{}/native-resume.bin", server.uri())],
            "downloadDir": temp.path(),
            "filename": "native-resume.bin",
            "segments": 1
        }))
        .send()
        .await
        .expect("create task request")
        .json()
        .await
        .expect("create task json");
    let task_id = created["taskId"].as_str().expect("task id").to_string();
    wait_for_task_progress_at_least(first_port, &task_id, 1024 * 1024).await;

    let saved: serde_json::Value = client
        .post(format!("http://127.0.0.1:{first_port}/api/v1/session/save"))
        .send()
        .await
        .expect("save session request")
        .json()
        .await
        .expect("save session json");
    assert_eq!(saved["status"], "saved");
    first.child.kill().expect("stop first daemon");
    wait_for_child_exit_after_forced_stop(&mut first).await;

    {
        let store = Store::open(&session_file).expect("store");
        let parsed_task_id = TaskId::parse(task_id.clone()).expect("task id parse");
        let native_segments = store
            .list_native_segments(&parsed_task_id)
            .expect("native segments");
        assert!(
            native_segments
                .iter()
                .any(|(_, segment)| segment.downloaded > 0),
            "expected native segment checkpoint progress"
        );
    }

    let second_port = allocate_port();
    let mut second = spawn_native_daemon(temp.path(), &session_file, second_port);
    wait_for_native_api_ready(second_port, &mut second)
        .await
        .expect("second daemon native API ready");

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let task: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{second_port}/api/v1/tasks/{task_id}"
            ))
            .send()
            .await
            .expect("task detail request")
            .json()
            .await
            .expect("task detail json");
        if task["lifecycle"] == "completed" {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "resumed native task never completed: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let requests = server.received_requests().await.expect("received requests");
    let saw_range = requests.iter().any(|request| {
        request.method.as_str() == "GET"
            && request.url.path() == "/native-resume.bin"
            && request.headers.get("range").is_some()
    });
    assert!(
        saw_range,
        "resumed daemon should issue a range request from native segment checkpoint"
    );
}
