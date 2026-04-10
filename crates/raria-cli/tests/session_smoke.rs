use std::net::TcpListener;
use std::path::Path;
use std::io::Read;
use std::process::{Child, Command, Stdio};
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

async fn rpc_call(port: u16, method_name: &str, params: serde_json::Value) -> serde_json::Value {
    reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method_name,
            "params": params,
        }))
        .send()
        .await
        .expect("send rpc request")
        .json()
        .await
        .expect("parse rpc response")
}

fn spawn_daemon(download_dir: &Path, session_file: &Path, rpc_port: u16, input_file: Option<&Path>) -> ChildGuard {
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

    if let Some(path) = input_file {
        cmd.arg("-i").arg(path);
    }

    ChildGuard {
        child: cmd.spawn().expect("spawn daemon"),
    }
}

fn spawn_daemon_with_extra_args(
    download_dir: &Path,
    session_file: &Path,
    rpc_port: u16,
    input_file: Option<&Path>,
    extra_args: &[&str],
) -> ChildGuard {
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

    if let Some(path) = input_file {
        cmd.arg("-i").arg(path);
    }
    for arg in extra_args {
        cmd.arg(arg);
    }

    ChildGuard {
        child: cmd.spawn().expect("spawn daemon"),
    }
}

async fn spawn_ready_daemon(
    download_dir: &Path,
    session_file: &Path,
    input_file: Option<&Path>,
) -> (ChildGuard, u16) {
    for _ in 0..8 {
        let rpc_port = allocate_port();
        let mut child = spawn_daemon(download_dir, session_file, rpc_port, input_file);
        match wait_for_rpc_ready_with_child(rpc_port, &mut child).await {
            Ok(()) => return (child, rpc_port),
            Err(message) if message.contains("failed to bind RPC server") => continue,
            Err(message) => panic!("{message}"),
        }
    }

    panic!("failed to start daemon on a free RPC port after multiple attempts");
}

async fn spawn_ready_daemon_with_args(
    download_dir: &Path,
    session_file: &Path,
    input_file: Option<&Path>,
    extra_args: &[&str],
) -> (ChildGuard, u16) {
    for _ in 0..8 {
        let rpc_port = allocate_port();
        let mut child = spawn_daemon_with_extra_args(download_dir, session_file, rpc_port, input_file, extra_args);
        match wait_for_rpc_ready_with_child(rpc_port, &mut child).await {
            Ok(()) => return (child, rpc_port),
            Err(message) if message.contains("failed to bind RPC server") => continue,
            Err(message) => panic!("{message}"),
        }
    }

    panic!("failed to start daemon on a free RPC port after multiple attempts");
}

async fn graceful_shutdown(port: u16, child: &mut ChildGuard) {
    let shutdown_resp = rpc_call(port, "aria2.shutdown", serde_json::json!([])).await;
    assert_eq!(shutdown_resp["result"], "OK");

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match child.child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "daemon exited unsuccessfully: {status}");
                return;
            }
            Ok(None) => {
                assert!(Instant::now() < deadline, "daemon did not exit after shutdown");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("failed waiting for daemon exit: {error}"),
        }
    }
}

#[tokio::test]
async fn daemon_restores_saved_job_after_restart() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/slow.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1048576")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/slow.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_bytes(vec![b'x'; 1024 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("saved.session.redb");

    let (mut first, first_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let add_resp = rpc_call(
        first_port,
        "aria2.addUri",
        serde_json::json!([[format!("{}/slow.bin", server.uri())]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status_resp = rpc_call(first_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if matches!(status, "waiting" | "active") {
            break;
        }

        assert!(Instant::now() < deadline, "job never reached a restorable state: {status_resp}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    graceful_shutdown(first_port, &mut first).await;
    assert!(session_file.is_file(), "session file should exist after graceful shutdown");

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let restored = rpc_call(second_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
    let restored_status = restored["result"]["status"].as_str().expect("restored status");
    assert!(
        matches!(restored_status, "waiting" | "active" | "complete"),
        "expected restored job to be present after restart, got {restored}"
    );

    graceful_shutdown(second_port, &mut second).await;
}

#[tokio::test]
async fn daemon_resume_after_restart_issues_range_request() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/resume-range.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/resume-range.bin"))
        .and(wiremock::matchers::header_exists("range"))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(vec![b'r'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/resume-range.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_bytes(vec![b'r'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("resume-range.session.redb");
    let (mut first, first_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--max-download-limit", "16384"],
    )
    .await;

    let add_resp = rpc_call(
        first_port,
        "aria2.addUri",
        serde_json::json!([
            [format!("{}/resume-range.bin", server.uri())],
            {
                "split": "1",
                "max-connection-per-server": "1"
            }
        ]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let progress_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(first_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let completed = status_resp["result"]["completedLength"]
            .as_str()
            .expect("completedLength string")
            .parse::<u64>()
            .expect("completedLength parse");
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "active" && completed > 0 {
            break;
        }

        assert!(
            Instant::now() < progress_deadline,
            "download never accumulated partial progress before shutdown: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    graceful_shutdown(first_port, &mut first).await;

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let completion_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status_resp = rpc_call(second_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "complete" {
            break;
        }

        assert!(
            Instant::now() < completion_deadline,
            "resumed job never completed after restart: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let requests = server.received_requests().await.expect("received requests");
    let saw_range = requests.iter().any(|req| {
        req.method.as_str() == "GET"
            && req.url.path() == "/resume-range.bin"
            && req.headers.get("range").is_some()
    });
    assert!(
        saw_range,
        "resumed daemon should issue at least one HTTP Range request after restart"
    );

    graceful_shutdown(second_port, &mut second).await;
}

#[tokio::test]
async fn daemon_resume_after_restart_sends_if_range_when_etag_is_known() {
    let server = MockServer::start().await;
    let etag = "\"resume-etag-123\"";

    Mock::given(method("HEAD"))
        .and(path("/resume-if-range.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes")
                .insert_header("etag", etag),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/resume-if-range.bin"))
        .and(wiremock::matchers::header_exists("range"))
        .and(wiremock::matchers::header("if-range", etag))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(vec![b'i'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/resume-if-range.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_bytes(vec![b'i'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("resume-if-range.session.redb");
    let (mut first, first_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--max-download-limit", "16384"],
    )
    .await;

    let add_resp = rpc_call(
        first_port,
        "aria2.addUri",
        serde_json::json!([
            [format!("{}/resume-if-range.bin", server.uri())],
            {
                "split": "1",
                "max-connection-per-server": "1"
            }
        ]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let progress_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(first_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let completed = status_resp["result"]["completedLength"]
            .as_str()
            .expect("completedLength string")
            .parse::<u64>()
            .expect("completedLength parse");
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "active" && completed > 0 {
            break;
        }
        assert!(
            Instant::now() < progress_deadline,
            "download never accumulated partial progress before shutdown: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    graceful_shutdown(first_port, &mut first).await;

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let completion_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status_resp = rpc_call(second_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "complete" {
            break;
        }

        assert!(
            Instant::now() < completion_deadline,
            "resumed job never completed after restart: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let requests = server.received_requests().await.expect("received requests");
    let saw_if_range = requests.iter().any(|req| {
        req.method.as_str() == "GET"
            && req.url.path() == "/resume-if-range.bin"
            && req.headers
                .get("if-range")
                .and_then(|v| v.to_str().ok())
                == Some(etag)
    });
    assert!(saw_if_range, "resumed daemon should send If-Range with the persisted ETag");

    graceful_shutdown(second_port, &mut second).await;
}

#[tokio::test]
async fn daemon_resume_after_restart_surfaces_non_zero_completed_length_before_completion() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/resume-visible.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/resume-visible.bin"))
        .and(wiremock::matchers::header_exists("range"))
        .respond_with(
            ResponseTemplate::new(206)
                .set_delay(Duration::from_secs(2))
                .set_body_bytes(vec![b'v'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/resume-visible.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_bytes(vec![b'v'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("resume-visible.session.redb");
    let (mut first, first_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--max-download-limit", "16384"],
    )
    .await;

    let add_resp = rpc_call(
        first_port,
        "aria2.addUri",
        serde_json::json!([
            [format!("{}/resume-visible.bin", server.uri())],
            {
                "split": "1",
                "max-connection-per-server": "1"
            }
        ]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let progress_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(first_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let completed = status_resp["result"]["completedLength"]
            .as_str()
            .expect("completedLength string")
            .parse::<u64>()
            .expect("completedLength parse");
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "active" && completed > 0 {
            break;
        }

        assert!(
            Instant::now() < progress_deadline,
            "download never accumulated partial progress before shutdown: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    graceful_shutdown(first_port, &mut first).await;

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let resumed_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(second_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let completed = status_resp["result"]["completedLength"]
            .as_str()
            .expect("completedLength string")
            .parse::<u64>()
            .expect("completedLength parse");
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "active" && completed > 0 {
            break;
        }
        if status == "complete" {
            panic!("resumed job completed before showing preserved non-zero completedLength: {status_resp}");
        }

        assert!(
            Instant::now() < resumed_deadline,
            "resumed daemon never surfaced preserved non-zero completedLength: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    graceful_shutdown(second_port, &mut second).await;
}

#[tokio::test]
async fn daemon_loads_jobs_from_input_file_on_startup() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/one.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/one.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_bytes(b"one1"),
        )
        .mount(&server)
        .await;

    Mock::given(method("HEAD"))
        .and(path("/two.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/two.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_bytes(b"two2"),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("input.session.redb");
    let input_file = temp.path().join("uris.txt");
    std::fs::write(
        &input_file,
        format!(
            "{base}/one.bin\n{base}/two.bin\t{base}/two.bin\n",
            base = server.uri()
        ),
    )
    .expect("write input file");

    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file, Some(&input_file)).await;

    let deadline = Instant::now() + Duration::from_secs(60);
    let jobs = loop {
        let active = rpc_call(rpc_port, "aria2.tellActive", serde_json::json!([])).await;
        let waiting = rpc_call(rpc_port, "aria2.tellWaiting", serde_json::json!([0, 10])).await;
        let stopped = rpc_call(rpc_port, "aria2.tellStopped", serde_json::json!([0, 10])).await;

        let mut jobs = active["result"].as_array().expect("active jobs array").clone();
        jobs.extend(
            waiting["result"]
                .as_array()
                .expect("waiting jobs array")
                .iter()
                .cloned(),
        );
        jobs.extend(
            stopped["result"]
                .as_array()
                .expect("stopped jobs array")
                .iter()
                .cloned(),
        );

        if jobs.len() >= 2 {
            break jobs;
        }

        assert!(
            Instant::now() < deadline,
            "daemon did not surface the input-file jobs through RPC in time"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    assert_eq!(jobs.len(), 2, "daemon should create one job per non-option URI line");

    let mut uri_counts = Vec::new();
    for job in &jobs {
        let uris = rpc_call(
            rpc_port,
            "aria2.getUris",
            serde_json::json!([job["gid"].as_str().expect("gid")]),
        )
        .await;
        uri_counts.push(uris["result"].as_array().expect("uris").len());
    }
    uri_counts.sort_unstable();
    assert_eq!(uri_counts, vec![1, 2]);

    graceful_shutdown(rpc_port, &mut child).await;
}

#[tokio::test]
async fn daemon_periodically_saves_session_when_interval_is_enabled() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/periodic.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/periodic.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'p'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("periodic.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--save-session-interval", "1"],
    )
    .await;

    let add_resp = rpc_call(
        rpc_port,
        "aria2.addUri",
        serde_json::json!([[format!("{}/periodic.bin", server.uri())]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let active_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(rpc_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if matches!(status, "waiting" | "active" | "complete") {
            break;
        }

        assert!(
            Instant::now() < active_deadline,
            "job never reached a savable state: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let save_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if session_file.is_file() && std::fs::metadata(&session_file).map(|m| m.len()).unwrap_or(0) > 0 {
            break;
        }

        assert!(
            Instant::now() < save_deadline,
            "daemon did not persist session file while running"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    graceful_shutdown(rpc_port, &mut child).await;
}

#[tokio::test]
async fn daemon_saves_session_when_save_session_rpc_is_called() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/rpc-save.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rpc-save.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'r'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("rpc-save.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let add_resp = rpc_call(
        rpc_port,
        "aria2.addUri",
        serde_json::json!([[format!("{}/rpc-save.bin", server.uri())]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let active_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(rpc_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if matches!(status, "waiting" | "active" | "complete") {
            break;
        }
        assert!(
            Instant::now() < active_deadline,
            "job never reached a savable state before aria2.saveSession: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let save_resp = rpc_call(rpc_port, "aria2.saveSession", serde_json::json!([])).await;
    assert_eq!(save_resp["result"], "OK");

    let save_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if session_file.is_file() && std::fs::metadata(&session_file).map(|m| m.len()).unwrap_or(0) > 0 {
            break;
        }
        assert!(
            Instant::now() < save_deadline,
            "daemon did not persist session file after aria2.saveSession"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    graceful_shutdown(rpc_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_saves_session_when_sigusr1_is_received() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/sigusr1.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/sigusr1.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b's'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("sigusr1.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let add_resp = rpc_call(
        rpc_port,
        "aria2.addUri",
        serde_json::json!([[format!("{}/sigusr1.bin", server.uri())]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let active_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(rpc_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if matches!(status, "waiting" | "active" | "complete") {
            break;
        }

        assert!(
            Instant::now() < active_deadline,
            "job never reached a savable state before SIGUSR1: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let daemon_pid = child.child.id() as i32;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(daemon_pid),
        nix::sys::signal::Signal::SIGUSR1,
    )
    .expect("send SIGUSR1");

    let save_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if session_file.is_file() && std::fs::metadata(&session_file).map(|m| m.len()).unwrap_or(0) > 0 {
            break;
        }

        assert!(
            Instant::now() < save_deadline,
            "daemon did not persist session file after SIGUSR1"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    graceful_shutdown(rpc_port, &mut child).await;
}

#[tokio::test]
async fn daemon_cli_headers_apply_to_input_file_downloads() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/daemon-header.bin"))
        .and(wiremock::matchers::header("x-daemon-header", "from-daemon"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/daemon-header.bin"))
        .and(wiremock::matchers::header("x-daemon-header", "from-daemon"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_bytes(b"done"),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("daemon-header.session.redb");
    let input_file = temp.path().join("uris.txt");
    std::fs::write(&input_file, format!("{}/daemon-header.bin\n", server.uri())).unwrap();

    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        Some(&input_file),
        &["--header", "X-Daemon-Header: from-daemon"],
    )
    .await;

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let active = rpc_call(rpc_port, "aria2.tellActive", serde_json::json!([])).await;
        let stopped = rpc_call(rpc_port, "aria2.tellStopped", serde_json::json!([0, 10])).await;

        if stopped["result"].as_array().unwrap().iter().any(|job| job["status"] == "complete") {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "daemon header-driven input-file job did not complete in time"
        );
        let _ = active;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(
        std::fs::read(temp.path().join("daemon-header.bin")).expect("read downloaded file"),
        b"done"
    );

    graceful_shutdown(rpc_port, &mut child).await;
}

#[tokio::test]
async fn daemon_cli_basic_auth_applies_to_input_file_downloads() {
    let server = MockServer::start().await;
    let auth_value = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"daemon-user:daemon-pass")
    );

    Mock::given(method("HEAD"))
        .and(path("/daemon-auth.bin"))
        .and(wiremock::matchers::header("authorization", auth_value.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/daemon-auth.bin"))
        .and(wiremock::matchers::header("authorization", auth_value.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_bytes(b"auth"),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("daemon-auth.session.redb");
    let input_file = temp.path().join("uris.txt");
    std::fs::write(&input_file, format!("{}/daemon-auth.bin\n", server.uri())).unwrap();

    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        Some(&input_file),
        &[
            "--http-user",
            "daemon-user",
            "--http-passwd",
            "daemon-pass",
        ],
    )
    .await;

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let stopped = rpc_call(rpc_port, "aria2.tellStopped", serde_json::json!([0, 10])).await;
        if stopped["result"].as_array().unwrap().iter().any(|job| job["status"] == "complete") {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "daemon auth-driven input-file job did not complete in time"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(
        std::fs::read(temp.path().join("daemon-auth.bin")).expect("read downloaded file"),
        b"auth"
    );

    graceful_shutdown(rpc_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_runs_on_download_start_hook() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/hook-start.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/hook-start.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b's'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("hook-start.session.redb");
    let hook_out = temp.path().join("start.hook.out");
    let script = temp.path().join("start-hook.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf \"%s|%s|%s\" \"$1\" \"$2\" \"$3\" > \"{}\"\n", hook_out.display()),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--on-download-start", script.to_string_lossy().as_ref()],
    )
    .await;

    let add_resp = rpc_call(
        rpc_port,
        "aria2.addUri",
        serde_json::json!([[format!("{}/hook-start.bin", server.uri())]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if hook_out.is_file() {
            break;
        }
        assert!(Instant::now() < deadline, "start hook did not run in time");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let hook_data = std::fs::read_to_string(&hook_out).expect("read hook output");
    assert!(hook_data.contains(&gid));
    assert!(hook_data.contains("|1|"));
    assert!(hook_data.contains("hook-start.bin"));

    graceful_shutdown(rpc_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_runs_on_download_complete_hook() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/hook-complete.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/hook-complete.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"done"))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("hook-complete.session.redb");
    let hook_out = temp.path().join("complete.hook.out");
    let script = temp.path().join("complete-hook.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf \"%s|%s|%s\" \"$1\" \"$2\" \"$3\" > \"{}\"\n", hook_out.display()),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--on-download-complete", script.to_string_lossy().as_ref()],
    )
    .await;

    let add_resp = rpc_call(
        rpc_port,
        "aria2.addUri",
        serde_json::json!([[format!("{}/hook-complete.bin", server.uri())]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(rpc_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "complete" && hook_out.is_file() {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "complete hook did not run in time: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let hook_data = std::fs::read_to_string(&hook_out).expect("read hook output");
    assert!(hook_data.contains(&gid));
    assert!(hook_data.contains("|1|"));
    assert!(hook_data.contains("hook-complete.bin"));

    graceful_shutdown(rpc_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_runs_on_download_error_hook() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/hook-error.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1024")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/hook-error.bin"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("hook-error.session.redb");
    let hook_out = temp.path().join("error.hook.out");
    let script = temp.path().join("error-hook.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf \"%s|%s|%s\" \"$1\" \"$2\" \"$3\" > \"{}\"\n", hook_out.display()),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let (mut child, rpc_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &[
            "--on-download-error",
            script.to_string_lossy().as_ref(),
            "--max-file-not-found",
            "1",
            "--max-tries",
            "10",
        ],
    )
    .await;

    let add_resp = rpc_call(
        rpc_port,
        "aria2.addUri",
        serde_json::json!([[format!("{}/hook-error.bin", server.uri())]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status_resp = rpc_call(rpc_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "error" && hook_out.is_file() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "error hook did not run in time: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let hook_data = std::fs::read_to_string(&hook_out).expect("read hook output");
    assert!(hook_data.contains(&gid));
    assert!(hook_data.contains("|1|"));
    assert!(hook_data.contains("hook-error.bin"));

    graceful_shutdown(rpc_port, &mut child).await;
}

#[tokio::test]
async fn daemon_fails_over_to_next_mirror_when_first_mirror_fails() {
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
        .respond_with(ResponseTemplate::new(503))
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
    let session_file = temp.path().join("mirror.session.redb");
    let (mut child, rpc_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let add_resp = rpc_call(
        rpc_port,
        "aria2.addUri",
        serde_json::json!([[
            format!("{}/mirror.bin", primary.uri()),
            format!("{}/mirror.bin", fallback.uri())
        ]]),
    )
    .await;
    let gid = add_resp["result"].as_str().expect("gid").to_string();

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status_resp = rpc_call(rpc_port, "aria2.tellStatus", serde_json::json!([gid.clone()])).await;
        let status = status_resp["result"]["status"].as_str().expect("status");
        if status == "complete" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "daemon never completed mirror failover job: {status_resp}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(std::fs::read(temp.path().join("mirror.bin")).unwrap(), b"pass");
    graceful_shutdown(rpc_port, &mut child).await;
}
