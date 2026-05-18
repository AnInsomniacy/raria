use std::io::Read;
use std::net::TcpListener;
use std::path::Path;
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

async fn wait_for_native_api_ready_with_child(
    port: u16,
    child: &mut ChildGuard,
) -> Result<(), String> {
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
            return Err(format!(
                "daemon native API did not become ready on port {port}"
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn native_post(port: u16, path: &str) -> serde_json::Value {
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}{path}"))
        .send()
        .await
        .expect("send native API request");
    assert!(
        response.status().is_success(),
        "native API request {path} should return success, got {}",
        response.status()
    );
    response.json().await.expect("parse native API response")
}

async fn native_get(port: u16, path: &str) -> serde_json::Value {
    let response = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}{path}"))
        .send()
        .await
        .expect("send native API request");
    assert!(
        response.status().is_success(),
        "native API request {path} should return success, got {}",
        response.status()
    );
    response.json().await.expect("parse native API response")
}

async fn create_native_task(
    port: u16,
    download_dir: &Path,
    source: String,
    filename: &str,
    segments: u32,
) -> String {
    create_native_task_with_sources(port, download_dir, vec![source], filename, segments).await
}

async fn create_native_task_with_sources(
    port: u16,
    download_dir: &Path,
    sources: Vec<String>,
    filename: &str,
    segments: u32,
) -> String {
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": sources,
            "downloadDir": download_dir,
            "filename": filename,
            "segments": segments
        }))
        .send()
        .await
        .expect("send native task creation request");
    assert!(
        response.status().is_success(),
        "native task creation should return success, got {}",
        response.status()
    );
    let created: serde_json::Value = response.json().await.expect("parse native task response");
    let task_id = created["taskId"].as_str().expect("task id").to_string();
    assert!(task_id.starts_with("task_"));
    assert!(!task_id.starts_with("task_migration_"));
    assert!(created.get("gid").is_none());
    assert!(created.get("jsonrpc").is_none());
    task_id
}

async fn create_native_task_with_checksum(
    port: u16,
    download_dir: &Path,
    source: String,
    filename: &str,
    checksum: &str,
) -> String {
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/api/v1/tasks"))
        .json(&serde_json::json!({
            "sources": [source],
            "downloadDir": download_dir,
            "filename": filename,
            "segments": 1,
            "checksum": checksum
        }))
        .send()
        .await
        .expect("send native task creation request");
    assert!(
        response.status().is_success(),
        "native task creation with checksum should return success, got {}",
        response.status()
    );
    let created: serde_json::Value = response.json().await.expect("parse native task response");
    let task_id = created["taskId"].as_str().expect("task id").to_string();
    assert!(task_id.starts_with("task_"));
    assert!(!task_id.starts_with("task_migration_"));
    assert!(created.get("gid").is_none());
    assert!(created.get("jsonrpc").is_none());
    task_id
}

async fn native_task(port: u16, task_id: &str) -> serde_json::Value {
    native_get(port, &format!("/api/v1/tasks/{task_id}")).await
}

async fn wait_for_native_task_progress(
    port: u16,
    task_id: &str,
    min_completed_bytes: u64,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let task = native_task(port, task_id).await;
        let completed = task["completedBytes"].as_u64().unwrap_or(0);
        if task["lifecycle"] == "running" && completed >= min_completed_bytes {
            return task;
        }

        assert!(
            Instant::now() < deadline,
            "native task never accumulated required partial progress: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_native_task_any_lifecycle(
    port: u16,
    task_id: &str,
    expected_lifecycles: &[&str],
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let task = native_task(port, task_id).await;
        let lifecycle = task["lifecycle"].as_str().expect("native lifecycle");
        if expected_lifecycles.contains(&lifecycle) {
            return task;
        }

        assert!(
            Instant::now() < deadline,
            "native task never reached one of {expected_lifecycles:?}: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_native_task_lifecycle(
    port: u16,
    task_id: &str,
    expected_lifecycle: &str,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let task = native_task(port, task_id).await;
        if task["lifecycle"] == expected_lifecycle {
            return task;
        }

        assert!(
            Instant::now() < deadline,
            "native task never reached lifecycle {expected_lifecycle}: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_native_task_count(port: u16, min_count: usize) -> Vec<serde_json::Value> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let response = native_get(port, "/api/v1/tasks").await;
        let tasks = response["tasks"].as_array().expect("tasks array").clone();
        if tasks.len() >= min_count {
            return tasks;
        }

        assert!(
            Instant::now() < deadline,
            "native task list never reached {min_count} tasks: {response}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_native_completed_task_count(
    port: u16,
    min_count: usize,
) -> Vec<serde_json::Value> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let tasks = wait_for_native_task_count(port, min_count).await;
        let completed = tasks
            .iter()
            .filter(|task| task["lifecycle"] == "completed")
            .count();
        if completed >= min_count {
            return tasks;
        }

        assert!(
            Instant::now() < deadline,
            "native task list never reached {min_count} completed tasks: {tasks:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn spawn_daemon(
    download_dir: &Path,
    session_file: &Path,
    api_port: u16,
    input_file: Option<&Path>,
) -> ChildGuard {
    let mut cmd = Command::new(cargo_bin("raria"));
    cmd.arg("daemon")
        .arg("-d")
        .arg(download_dir)
        .arg("--api-port")
        .arg(api_port.to_string())
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
    api_port: u16,
    input_file: Option<&Path>,
    extra_args: &[&str],
) -> ChildGuard {
    let mut cmd = Command::new(cargo_bin("raria"));
    cmd.arg("daemon")
        .arg("-d")
        .arg(download_dir)
        .arg("--api-port")
        .arg(api_port.to_string())
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
        let api_port = allocate_port();
        let mut child = spawn_daemon(download_dir, session_file, api_port, input_file);
        match wait_for_native_api_ready_with_child(api_port, &mut child).await {
            Ok(()) => return (child, api_port),
            Err(message) if message.contains("failed to bind API server") => continue,
            Err(message) => panic!("{message}"),
        }
    }

    panic!("failed to start daemon on a free API port after multiple attempts");
}

async fn spawn_ready_daemon_with_args(
    download_dir: &Path,
    session_file: &Path,
    input_file: Option<&Path>,
    extra_args: &[&str],
) -> (ChildGuard, u16) {
    for _ in 0..8 {
        let api_port = allocate_port();
        let mut child = spawn_daemon_with_extra_args(
            download_dir,
            session_file,
            api_port,
            input_file,
            extra_args,
        );
        match wait_for_native_api_ready_with_child(api_port, &mut child).await {
            Ok(()) => return (child, api_port),
            Err(message) if message.contains("failed to bind API server") => continue,
            Err(message) => panic!("{message}"),
        }
    }

    panic!("failed to start daemon on a free API port after multiple attempts");
}

async fn graceful_shutdown(port: u16, child: &mut ChildGuard) {
    let shutdown_resp = native_post(port, "/api/v1/daemon/shutdown").await;
    assert_eq!(shutdown_resp["status"], "shuttingDown");
    assert!(shutdown_resp.get("jsonrpc").is_none());
    assert!(shutdown_resp.get("result").is_none());

    let deadline = Instant::now() + Duration::from_secs(60);
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

    let task_id = create_native_task(
        first_port,
        temp.path(),
        format!("{}/slow.bin", server.uri()),
        "slow.bin",
        1,
    )
    .await;
    wait_for_native_task_any_lifecycle(first_port, &task_id, &["queued", "running", "completed"])
        .await;

    graceful_shutdown(first_port, &mut first).await;
    assert!(
        session_file.is_file(),
        "session file should exist after graceful shutdown"
    );

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let restored = native_task(second_port, &task_id).await;
    let restored_status = restored["lifecycle"].as_str().expect("restored lifecycle");
    assert!(
        matches!(restored_status, "queued" | "running" | "completed"),
        "expected restored native task to be present after restart, got {restored}"
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
        .respond_with(ResponseTemplate::new(206).set_body_bytes(vec![b'r'; 256 * 1024]))
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

    let task_id = create_native_task(
        first_port,
        temp.path(),
        format!("{}/resume-range.bin", server.uri()),
        "resume-range.bin",
        1,
    )
    .await;

    wait_for_native_task_progress(first_port, &task_id, 1).await;

    graceful_shutdown(first_port, &mut first).await;

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    wait_for_native_task_lifecycle(second_port, &task_id, "completed").await;

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
        .respond_with(ResponseTemplate::new(206).set_body_bytes(vec![b'i'; 256 * 1024]))
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

    let task_id = create_native_task(
        first_port,
        temp.path(),
        format!("{}/resume-if-range.bin", server.uri()),
        "resume-if-range.bin",
        1,
    )
    .await;
    wait_for_native_task_progress(first_port, &task_id, 1).await;

    graceful_shutdown(first_port, &mut first).await;

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    wait_for_native_task_lifecycle(second_port, &task_id, "completed").await;

    let requests = server.received_requests().await.expect("received requests");
    let saw_if_range = requests.iter().any(|req| {
        req.method.as_str() == "GET"
            && req.url.path() == "/resume-if-range.bin"
            && req.headers.get("if-range").and_then(|v| v.to_str().ok()) == Some(etag)
    });
    assert!(
        saw_if_range,
        "resumed daemon should send If-Range with the persisted ETag"
    );

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

    let task_id = create_native_task(
        first_port,
        temp.path(),
        format!("{}/resume-visible.bin", server.uri()),
        "resume-visible.bin",
        1,
    )
    .await;
    wait_for_native_task_progress(first_port, &task_id, 1).await;

    graceful_shutdown(first_port, &mut first).await;

    let (mut second, second_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let resumed_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let task = native_task(second_port, &task_id).await;
        let completed = task["completedBytes"].as_u64().unwrap_or(0);
        let lifecycle = task["lifecycle"].as_str().expect("native lifecycle");
        if lifecycle == "running" && completed > 0 {
            break;
        }
        if lifecycle == "completed" {
            panic!(
                "resumed native task completed before showing preserved non-zero completedBytes: {task}"
            );
        }

        assert!(
            Instant::now() < resumed_deadline,
            "resumed daemon never surfaced preserved non-zero completedBytes: {task}"
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

    let (mut child, api_port) =
        spawn_ready_daemon(temp.path(), &session_file, Some(&input_file)).await;

    let jobs = wait_for_native_task_count(api_port, 2).await;

    assert_eq!(
        jobs.len(),
        2,
        "daemon should create one job per non-option URI line"
    );

    let mut uri_counts = Vec::new();
    for job in &jobs {
        uri_counts.push(job["sources"].as_array().expect("sources").len());
    }
    uri_counts.sort_unstable();
    assert_eq!(uri_counts, vec![1, 2]);

    graceful_shutdown(api_port, &mut child).await;
}

#[tokio::test]
async fn daemon_conditional_get_skips_download_when_remote_is_not_modified() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/cached.bin"))
        .and(wiremock::matchers::header_exists("if-modified-since"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("conditional-get.session.redb");
    let cached_path = temp.path().join("cached.bin");
    std::fs::write(&cached_path, b"cached-copy").expect("write cached copy");

    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--conditional-get", "--allow-overwrite"],
    )
    .await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/cached.bin", server.uri()),
        "cached.bin",
        1,
    )
    .await;

    wait_for_native_task_lifecycle(api_port, &task_id, "completed").await;
    assert_eq!(
        std::fs::read(&cached_path).expect("read cached file"),
        b"cached-copy"
    );

    graceful_shutdown(api_port, &mut child).await;
}

#[tokio::test]
async fn daemon_rejects_checksum_mismatch_before_marking_job_complete() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/checksum.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/checksum.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"good"))
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("checksum.session.redb");
    let (mut child, api_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let task_id = create_native_task_with_checksum(
        api_port,
        temp.path(),
        format!("{}/checksum.bin", server.uri()),
        "checksum.bin",
        "sha-256=0000000000000000000000000000000000000000000000000000000000000000",
    )
    .await;

    let status_resp = wait_for_native_task_lifecycle(api_port, &task_id, "failed").await;
    let error_message = status_resp["errorMessage"].as_str().expect("error message");
    assert!(
        error_message.contains("checksum"),
        "checksum mismatch should surface in daemon status: {status_resp}"
    );

    let output_path = temp.path().join("checksum.bin");
    assert!(
        !output_path.exists(),
        "checksum mismatch should remove the invalid output file"
    );

    graceful_shutdown(api_port, &mut child).await;
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
    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--save-session-interval", "1"],
    )
    .await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/periodic.bin", server.uri()),
        "periodic.bin",
        1,
    )
    .await;
    wait_for_native_task_any_lifecycle(api_port, &task_id, &["queued", "running", "completed"])
        .await;

    let save_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if session_file.is_file()
            && std::fs::metadata(&session_file)
                .map(|m| m.len())
                .unwrap_or(0)
                > 0
        {
            break;
        }

        assert!(
            Instant::now() < save_deadline,
            "daemon did not persist session file while running"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    graceful_shutdown(api_port, &mut child).await;
}

#[tokio::test]
async fn daemon_saves_session_when_native_save_session_is_called() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/native-save.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "262144")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/native-save.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_bytes(vec![b'r'; 256 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("native-save.session.redb");
    let (mut child, api_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/native-save.bin", server.uri()),
        "native-save.bin",
        1,
    )
    .await;
    wait_for_native_task_any_lifecycle(api_port, &task_id, &["queued", "running", "completed"])
        .await;

    let save_resp = native_post(api_port, "/api/v1/session/save").await;
    assert_eq!(save_resp["status"], "saved");
    assert_eq!(save_resp["sessionPath"].as_str(), session_file.to_str());
    assert!(save_resp.get("jsonrpc").is_none());
    assert!(save_resp.get("result").is_none());

    let save_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if session_file.is_file()
            && std::fs::metadata(&session_file)
                .map(|m| m.len())
                .unwrap_or(0)
                > 0
        {
            break;
        }
        assert!(
            Instant::now() < save_deadline,
            "daemon did not persist session file after native session save"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    graceful_shutdown(api_port, &mut child).await;
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
    let (mut child, api_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/sigusr1.bin", server.uri()),
        "sigusr1.bin",
        1,
    )
    .await;
    wait_for_native_task_any_lifecycle(api_port, &task_id, &["queued", "running", "completed"])
        .await;

    let daemon_pid = child.child.id() as i32;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(daemon_pid),
        nix::sys::signal::Signal::SIGUSR1,
    )
    .expect("send SIGUSR1");

    let save_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if session_file.is_file()
            && std::fs::metadata(&session_file)
                .map(|m| m.len())
                .unwrap_or(0)
                > 0
        {
            break;
        }

        assert!(
            Instant::now() < save_deadline,
            "daemon did not persist session file after SIGUSR1"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    graceful_shutdown(api_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_sigterm_shuts_down_promptly_while_throttled() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/sigterm-throttled.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "524288")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sigterm-throttled.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(500))
                .set_body_bytes(vec![b't'; 512 * 1024]),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("sigterm-throttled.session.redb");
    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--max-download-limit", "16384"],
    )
    .await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/sigterm-throttled.bin", server.uri()),
        "sigterm-throttled.bin",
        1,
    )
    .await;
    wait_for_native_task_progress(api_port, &task_id, 1).await;

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .expect("send SIGTERM");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.child.try_wait() {
            Ok(Some(status)) => {
                assert!(
                    status.success(),
                    "daemon exited unsuccessfully after SIGTERM: {status}"
                );
                break;
            }
            Ok(None) => {
                assert!(
                    Instant::now() < deadline,
                    "daemon did not exit promptly after SIGTERM while throttled"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("failed waiting for SIGTERM shutdown: {error}"),
        }
    }
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

    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        Some(&input_file),
        &["--header", "X-Daemon-Header: from-daemon"],
    )
    .await;

    wait_for_native_completed_task_count(api_port, 1).await;

    assert_eq!(
        std::fs::read(temp.path().join("daemon-header.bin")).expect("read downloaded file"),
        b"done"
    );

    graceful_shutdown(api_port, &mut child).await;
}

#[tokio::test]
async fn daemon_input_file_per_uri_headers_apply_to_downloads() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/input-header.bin"))
        .and(wiremock::matchers::header("x-input-header", "from-input"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "5")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/input-header.bin"))
        .and(wiremock::matchers::header("x-input-header", "from-input"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_bytes(b"input"),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let session_file = temp.path().join("input-header.session.redb");
    let input_file = temp.path().join("uris.txt");
    std::fs::write(
        &input_file,
        format!(
            "{}/input-header.bin\n  header=X-Input-Header: from-input\n",
            server.uri()
        ),
    )
    .unwrap();

    let (mut child, api_port) =
        spawn_ready_daemon(temp.path(), &session_file, Some(&input_file)).await;

    wait_for_native_completed_task_count(api_port, 1).await;

    assert_eq!(
        std::fs::read(temp.path().join("input-header.bin")).expect("read downloaded file"),
        b"input"
    );

    graceful_shutdown(api_port, &mut child).await;
}

#[tokio::test]
async fn daemon_cli_basic_auth_applies_to_input_file_downloads() {
    let server = MockServer::start().await;
    let auth_value = format!(
        "Basic {}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"daemon-user:daemon-pass"
        )
    );

    Mock::given(method("HEAD"))
        .and(path("/daemon-auth.bin"))
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
        .and(path("/daemon-auth.bin"))
        .and(wiremock::matchers::header(
            "authorization",
            auth_value.as_str(),
        ))
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

    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        Some(&input_file),
        &["--http-user", "daemon-user", "--http-passwd", "daemon-pass"],
    )
    .await;

    wait_for_native_completed_task_count(api_port, 1).await;

    assert_eq!(
        std::fs::read(temp.path().join("daemon-auth.bin")).expect("read downloaded file"),
        b"auth"
    );

    graceful_shutdown(api_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_runs_on_task_start_hook() {
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
        format!(
            "#!/bin/sh\nprintf \"%s|%s|%s\" \"$1\" \"$2\" \"$3\" > \"{}\"\n",
            hook_out.display()
        ),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--on-task-start", script.to_string_lossy().as_ref()],
    )
    .await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/hook-start.bin", server.uri()),
        "hook-start.bin",
        1,
    )
    .await;

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if hook_out.is_file() {
            break;
        }
        assert!(Instant::now() < deadline, "start hook did not run in time");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let hook_data = std::fs::read_to_string(&hook_out).expect("read hook output");
    assert!(hook_data.contains(&task_id));
    assert!(hook_data.contains("|1|"));
    assert!(hook_data.contains("hook-start.bin"));

    graceful_shutdown(api_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_runs_on_task_complete_hook() {
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
        format!(
            "#!/bin/sh\nprintf \"%s|%s|%s\" \"$1\" \"$2\" \"$3\" > \"{}\"\n",
            hook_out.display()
        ),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &["--on-task-complete", script.to_string_lossy().as_ref()],
    )
    .await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/hook-complete.bin", server.uri()),
        "hook-complete.bin",
        1,
    )
    .await;

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let task = native_task(api_port, &task_id).await;
        let lifecycle = task["lifecycle"].as_str().expect("native lifecycle");
        if lifecycle == "completed" && hook_out.is_file() {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "complete hook did not run in time: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let hook_data = std::fs::read_to_string(&hook_out).expect("read hook output");
    assert!(hook_data.contains(&task_id));
    assert!(hook_data.contains("|1|"));
    assert!(hook_data.contains("hook-complete.bin"));

    graceful_shutdown(api_port, &mut child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_runs_on_task_fail_hook() {
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
        format!(
            "#!/bin/sh\nprintf \"%s|%s|%s\" \"$1\" \"$2\" \"$3\" > \"{}\"\n",
            hook_out.display()
        ),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let (mut child, api_port) = spawn_ready_daemon_with_args(
        temp.path(),
        &session_file,
        None,
        &[
            "--on-task-fail",
            script.to_string_lossy().as_ref(),
            "--max-file-not-found",
            "1",
            "--max-tries",
            "10",
        ],
    )
    .await;

    let task_id = create_native_task(
        api_port,
        temp.path(),
        format!("{}/hook-error.bin", server.uri()),
        "hook-error.bin",
        1,
    )
    .await;

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let task = native_task(api_port, &task_id).await;
        let lifecycle = task["lifecycle"].as_str().expect("native lifecycle");
        if lifecycle == "failed" && hook_out.is_file() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "error hook did not run in time: {task}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let hook_data = std::fs::read_to_string(&hook_out).expect("read hook output");
    assert!(hook_data.contains(&task_id));
    assert!(hook_data.contains("|1|"));
    assert!(hook_data.contains("hook-error.bin"));

    graceful_shutdown(api_port, &mut child).await;
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
    let (mut child, api_port) = spawn_ready_daemon(temp.path(), &session_file, None).await;

    let task_id = create_native_task_with_sources(
        api_port,
        temp.path(),
        vec![
            format!("{}/mirror.bin", primary.uri()),
            format!("{}/mirror.bin", fallback.uri()),
        ],
        "mirror.bin",
        1,
    )
    .await;

    wait_for_native_task_lifecycle(api_port, &task_id, "completed").await;

    assert_eq!(
        std::fs::read(temp.path().join("mirror.bin")).unwrap(),
        b"pass"
    );
    graceful_shutdown(api_port, &mut child).await;
}
