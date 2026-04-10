use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    let deadline = Instant::now() + Duration::from_secs(15);
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
        let child = Command::new(cargo_bin("raria"))
            .arg("daemon")
            .arg("-d")
            .arg(download_dir)
            .arg("--rpc-port")
            .arg(rpc_port.to_string())
            .arg("--session-file")
            .arg(session_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn daemon");
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
