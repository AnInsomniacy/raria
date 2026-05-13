use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures::StreamExt;
use raria_core::native::TaskId;
use raria_core::persist::Store;
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
        .arg("-d")
        .arg(download_dir)
        .arg("--api-port")
        .arg(port.to_string())
        .arg("--session-file")
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
    assert!(!task_id.starts_with("task_migration_"));
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
        std::ffi::OsString::from("--conf-path"),
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
        std::ffi::OsString::from("--max-download-limit"),
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
