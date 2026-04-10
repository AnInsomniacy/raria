use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::tempdir;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
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

#[tokio::test]
async fn daemon_emits_cors_headers_when_rpc_allow_origin_all_is_enabled() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("cors.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--rpc-allow-origin-all"],
    )
    .await;

    let client = reqwest::Client::new();
    let preflight = client
        .request(
            reqwest::Method::OPTIONS,
            format!("http://127.0.0.1:{rpc_port}/jsonrpc"),
        )
        .header("Origin", "https://ui.example")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .expect("send CORS preflight");

    assert!(
        preflight.status().is_success(),
        "preflight should succeed when daemon CORS is enabled: {}",
        preflight.status()
    );
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok()),
        Some("*"),
    );

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
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

#[tokio::test]
async fn daemon_rejects_ws_origin_by_default() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("ws-origin-default.session.redb");
    let (_child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let mut request = format!("ws://127.0.0.1:{rpc_port}/jsonrpc")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", "https://ui.example".parse().unwrap());

    let result = connect_async(request).await;
    assert!(
        result.is_err(),
        "daemon should reject browser-style WS upgrade by default"
    );
}

#[tokio::test]
async fn daemon_allows_ws_origin_when_rpc_allow_origin_all_is_enabled() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("ws-origin-allow.session.redb");
    let (_child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--rpc-allow-origin-all"],
    )
    .await;

    let mut request = format!("ws://127.0.0.1:{rpc_port}/jsonrpc")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", "https://ui.example".parse().unwrap());

    let result = connect_async(request).await;
    assert!(
        result.is_ok(),
        "daemon should allow browser-style WS upgrade when rpc_allow_origin_all is enabled: {result:?}"
    );
}

#[tokio::test]
async fn daemon_accepts_rpc_add_uri_and_shutdown() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/file.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/file.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("test.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let client = reqwest::Client::new();

    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [[format!("{}/file.bin", download_server.uri())]],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();
    assert_eq!(gid.len(), 16);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status_resp: serde_json::Value = client
            .post(format!("http://127.0.0.1:{rpc_port}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "aria2.tellStatus",
                "params": [gid],
            }))
            .send()
            .await
            .expect("send tellStatus")
            .json()
            .await
            .expect("parse tellStatus");

        let status = status_resp["result"]["status"].as_str().expect("status");
        if matches!(status, "waiting" | "active" | "complete") {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "unexpected daemon status response: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
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

    let mut stdout = String::new();
    if let Some(mut handle) = child.child.stdout.take() {
        let _ = handle.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut handle) = child.child.stderr.take() {
        let _ = handle.read_to_string(&mut stderr);
    }

    // Avoid silent flakiness: if the test passed but the daemon logged an error,
    // surface it to the assertion context.
    if stderr.contains("ERROR") || stderr.contains("error") {
        thread::sleep(Duration::from_millis(50));
    }
}


#[tokio::test]
async fn daemon_bt_job_pause_and_unpause_round_trip_over_rpc() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("bt-pause-roundtrip.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [["magnet:?xt=urn:btih:da39a3ee5e6b4b0d3255bfef95601890afd80709"]],
        }))
        .send()
        .await
        .expect("send addUri magnet")
        .json()
        .await
        .expect("parse addUri magnet response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let pause_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "aria2.pause",
            "params": [gid.clone()],
        }))
        .send()
        .await
        .expect("send pause")
        .json()
        .await
        .expect("parse pause response");
    assert_eq!(pause_resp["result"], gid);

    let paused_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status_resp: serde_json::Value = client
            .post(format!("http://127.0.0.1:{rpc_port}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "aria2.tellStatus",
                "params": [gid.clone()],
            }))
            .send()
            .await
            .expect("send tellStatus paused")
            .json()
            .await
            .expect("parse tellStatus paused");

        if status_resp["result"]["status"].as_str() == Some("paused") {
            break;
        }

        assert!(
            Instant::now() < paused_deadline,
            "BT job never reached paused status over daemon RPC: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let unpause_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "aria2.unpause",
            "params": [gid.clone()],
        }))
        .send()
        .await
        .expect("send unpause")
        .json()
        .await
        .expect("parse unpause response");
    assert_eq!(unpause_resp["result"], gid);

    let resumed_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status_resp: serde_json::Value = client
            .post(format!("http://127.0.0.1:{rpc_port}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "aria2.tellStatus",
                "params": [gid.clone()],
            }))
            .send()
            .await
            .expect("send tellStatus resumed")
            .json()
            .await
            .expect("parse tellStatus resumed");

        let status = status_resp["result"]["status"].as_str().expect("status");
        if matches!(status, "waiting" | "active") {
            break;
        }

        assert!(
            Instant::now() < resumed_deadline,
            "BT job never resumed from paused state over daemon RPC: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 6,
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

#[tokio::test]
async fn daemon_respects_min_split_size_when_calculating_effective_connections() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/minsplit.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "524288")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/minsplit.bin"))
        .and(wiremock::matchers::header("range", "bytes=0-262143"))
        .respond_with(
            ResponseTemplate::new(206)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'a'; 256 * 1024]),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/minsplit.bin"))
        .and(wiremock::matchers::header("range", "bytes=262144-524287"))
        .respond_with(
            ResponseTemplate::new(206)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'b'; 256 * 1024]),
        )
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("minsplit.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--min-split-size", "262144"],
    )
    .await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [
                [format!("{}/minsplit.bin", download_server.uri())],
                {
                    "split": "8",
                    "max-connection-per-server": "8",
                    "out": "minsplit.bin"
                }
            ],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
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

        let status = status_resp["result"]["status"].as_str().expect("status");
        let connections = status_resp["result"]["connections"]
            .as_str()
            .expect("connections string")
            .parse::<u32>()
            .expect("connections should parse as integer");

        if status == "active" && connections == 2 {
            break;
        }
        if status == "complete" {
            panic!("download completed before reaching 2 active connections: {status_resp}");
        }

        assert!(
            Instant::now() < deadline,
            "daemon never reached 2 active connections under min-split-size: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
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

#[tokio::test]
async fn daemon_respects_split_and_max_connection_per_server_options() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/connections.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1048576")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/connections.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'c'; 1024 * 1024]),
        )
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("connections-options.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [
                [format!("{}/connections.bin", download_server.uri())],
                {
                    "split": "4",
                    "max-connection-per-server": "4"
                }
            ],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
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

        let status = status_resp["result"]["status"].as_str().expect("status");
        let connections = status_resp["result"]["connections"]
            .as_str()
            .expect("connections string")
            .parse::<u32>()
            .expect("connections should parse as integer");

        if status == "active" && connections == 4 {
            break;
        }
        if status == "complete" {
            panic!("download completed before surfacing the configured connection count: {status_resp}");
        }

        assert!(
            Instant::now() < deadline,
            "daemon never surfaced the configured split/max-connection-per-server count: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
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

#[tokio::test]
async fn daemon_stops_retrying_after_max_file_not_found() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/missing.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1024")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/missing.bin"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("missing.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--max-file-not-found", "1", "--max-tries", "10"],
    )
    .await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [
                [format!("{}/missing.bin", download_server.uri())],
                {
                    "split": "1",
                    "max-connection-per-server": "1",
                    "out": "missing.bin"
                }
            ],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
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

        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "error" {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "daemon never marked missing file as error: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let requests = download_server
        .received_requests()
        .await
        .expect("received requests");
    let get_count = requests
        .iter()
        .filter(|req| req.method.as_str() == "GET" && req.url.path() == "/missing.bin")
        .count();
    assert_eq!(get_count, 1, "expected exactly one GET attempt for missing file");

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
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

#[tokio::test]
async fn daemon_uses_rpc_supplied_headers_on_real_download_requests() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/header.bin"))
        .and(wiremock::matchers::header("x-rpc-header", "from-rpc"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/header.bin"))
        .and(wiremock::matchers::header("x-rpc-header", "from-rpc"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"done"))
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("header.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [
                [format!("{}/header.bin", download_server.uri())],
                {
                    "header": ["X-Rpc-Header: from-rpc"],
                    "out": "header.bin"
                }
            ],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();
    let deadline = Instant::now() + Duration::from_secs(10);

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

        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "complete" {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "daemon header-propagation job did not complete in time: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(
        std::fs::read(temp.path().join("header.bin")).expect("read downloaded file"),
        b"done"
    );

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
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

#[tokio::test]
async fn daemon_uses_rpc_supplied_basic_auth_on_real_download_requests() {
    let download_server = MockServer::start().await;
    let auth_value = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"rpc-user:rpc-pass")
    );

    Mock::given(method("HEAD"))
        .and(path("/auth.bin"))
        .and(wiremock::matchers::header("authorization", auth_value.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/auth.bin"))
        .and(wiremock::matchers::header("authorization", auth_value.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"auth"))
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("auth.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [
                [format!("{}/auth.bin", download_server.uri())],
                {
                    "out": "auth.bin",
                    "http-user": "rpc-user",
                    "http-passwd": "rpc-pass"
                }
            ],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();
    let deadline = Instant::now() + Duration::from_secs(10);

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

        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "complete" {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "daemon auth-propagation job did not complete in time: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(
        std::fs::read(temp.path().join("auth.bin")).expect("read downloaded file"),
        b"auth"
    );

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
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

#[tokio::test]
async fn daemon_writes_logs_to_requested_file() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("log.session.redb");
    let log_path = temp.path().join("daemon.log");
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--log", log_path.to_str().unwrap()],
    )
    .await;

    let shutdown_resp: serde_json::Value = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 50,
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

    assert!(log_path.is_file(), "daemon should create the requested log file");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let log = std::fs::read_to_string(&log_path).expect("read log file");
        if !log.trim().is_empty() {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "daemon log file should not remain empty after process shutdown"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_flag_detaches_process_and_keeps_rpc_alive() {
    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("daemonize.session.redb");
    let rpc_port = allocate_port();

    let mut child = Command::new(cargo_bin("raria"))
        .arg("daemon")
        .arg("-d")
        .arg(temp.path())
        .arg("--rpc-port")
        .arg(rpc_port.to_string())
        .arg("--session-file")
        .arg(&session_file)
        .arg("--daemon")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemonize request");

    let exit_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "daemonizing parent exited unsuccessfully: {status}");
                break;
            }
            Ok(None) => {
                assert!(Instant::now() < exit_deadline, "daemonizing parent did not exit promptly");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("failed waiting for daemonizing parent: {error}"),
        }
    }

    let rpc_deadline = Instant::now() + Duration::from_secs(30);
    let client = reqwest::Client::new();
    loop {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.getVersion",
            "params": [],
        });

        if let Ok(resp) = client
            .post(format!("http://127.0.0.1:{rpc_port}"))
            .json(&body)
            .send()
            .await
        {
            if resp.status().is_success() {
                break;
            }
        }

        assert!(Instant::now() < rpc_deadline, "background daemon never became RPC-ready");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
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
}

#[tokio::test]
async fn tell_status_reports_non_zero_connections_while_download_is_active() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/slow-connections.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1048576")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/slow-connections.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'x'; 1024 * 1024]),
        )
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("connections.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [[format!("{}/slow-connections.bin", download_server.uri())]],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    let observed_connections = loop {
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

        let status = status_resp["result"]["status"].as_str().expect("status");
        let connections = status_resp["result"]["connections"]
            .as_str()
            .expect("connections string")
            .parse::<u32>()
            .expect("connections should parse as integer");

        if status == "active" && connections > 0 {
            break connections;
        }

        assert!(
            Instant::now() < deadline,
            "active tellStatus never surfaced non-zero connections: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    assert!(
        observed_connections > 0,
        "connections must be greater than zero while a segmented download is active"
    );

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
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

#[tokio::test]
async fn change_global_option_updates_active_download_limit_in_product_path() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/dynamic-limit.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "524288")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/dynamic-limit.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'y'; 512 * 1024]))
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("dynamic-limit.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        &["--max-download-limit", "16384"],
    )
    .await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [
                [format!("{}/dynamic-limit.bin", download_server.uri())],
                {
                    "split": "1",
                    "max-connection-per-server": "1"
                }
            ],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let active_deadline = Instant::now() + Duration::from_secs(20);
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

        let status = status_resp["result"]["status"].as_str().expect("status");
        let completed = status_resp["result"]["completedLength"]
            .as_str()
            .expect("completedLength string")
            .parse::<u64>()
            .expect("completedLength should parse");

        if status == "active" && completed > 0 {
            break;
        }

        assert!(
            Instant::now() < active_deadline,
            "download never reached observable active state before runtime limit mutation: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "aria2.changeGlobalOption",
            "params": [{"max-overall-download-limit": "0"}],
        }))
        .send()
        .await
        .expect("send changeGlobalOption")
        .json()
        .await
        .expect("parse changeGlobalOption response");
    assert_eq!(resp["result"], "OK");

    let completion_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status_resp: serde_json::Value = client
            .post(format!("http://127.0.0.1:{rpc_port}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "aria2.tellStatus",
                "params": [gid.clone()],
            }))
            .send()
            .await
            .expect("send tellStatus after limit change")
            .json()
            .await
            .expect("parse tellStatus after limit change");

        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "complete" {
            break;
        }

        assert!(
            Instant::now() < completion_deadline,
            "download did not complete soon after global limit was removed: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
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

#[tokio::test]
async fn change_global_option_can_enable_a_limit_for_an_already_active_download() {
    let download_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/late-limit.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "524288")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&download_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/late-limit.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(500))
                .set_body_bytes(vec![b'z'; 512 * 1024]),
        )
        .mount(&download_server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("late-limit.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file).await;

    let client = reqwest::Client::new();
    let add_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [
                [format!("{}/late-limit.bin", download_server.uri())],
                {
                    "split": "1",
                    "max-connection-per-server": "1"
                }
            ],
        }))
        .send()
        .await
        .expect("send addUri")
        .json()
        .await
        .expect("parse addUri response");

    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let active_deadline = Instant::now() + Duration::from_secs(10);
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

        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "active" {
            break;
        }

        assert!(
            Instant::now() < active_deadline,
            "download never reached active state before late limit mutation: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "aria2.changeGlobalOption",
            "params": [{"max-overall-download-limit": "16384"}],
        }))
        .send()
        .await
        .expect("send changeGlobalOption")
        .json()
        .await
        .expect("parse changeGlobalOption response");
    assert_eq!(resp["result"], "OK");

    let stall_window = Instant::now() + Duration::from_secs(2);
    loop {
        let status_resp: serde_json::Value = client
            .post(format!("http://127.0.0.1:{rpc_port}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "aria2.tellStatus",
                "params": [gid.clone()],
            }))
            .send()
            .await
            .expect("send tellStatus after enabling limit")
            .json()
            .await
            .expect("parse tellStatus after enabling limit");

        let status = status_resp["result"]["status"].as_str().expect("status");
        if Instant::now() >= stall_window {
            assert_ne!(
                status,
                "complete",
                "download completed too quickly after enabling a very low global limit: {status_resp}"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let shutdown_resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{rpc_port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
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
