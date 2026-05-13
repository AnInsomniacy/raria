#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use raria_core::config::GlobalConfig;
    use raria_core::engine::{AddUriSpec, Engine};
    use raria_core::job::Gid;
    use raria_core::native::TaskId;
    use raria_core::progress::DownloadEvent;
    use raria_rpc::api::{NativeApiConfig, start_native_api_server};
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn health_endpoint_returns_native_api_envelope() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let body: serde_json::Value = reqwest::get(format!("http://{}/api/v1/health", addrs.http))
            .await
            .expect("health request")
            .json()
            .await
            .expect("health json");

        assert_eq!(body["status"], "ok");
        assert_eq!(body["apiVersion"], 1);
        assert!(body.get("jsonrpc").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn tasks_endpoint_returns_native_task_projection() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add task");

        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let body: serde_json::Value = reqwest::get(format!("http://{}/api/v1/tasks", addrs.http))
            .await
            .expect("tasks request")
            .json()
            .await
            .expect("tasks json");

        let tasks = body["tasks"].as_array().expect("tasks array");
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0]["taskId"].as_str().unwrap().starts_with("task_"));
        assert_eq!(tasks[0]["lifecycle"], "queued");
        assert_eq!(tasks[0]["sources"][0]["protocol"], "https");
        assert!(tasks[0].get("gid").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_detail_pause_and_resume_use_native_task_id() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add task");

        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");
        let client = reqwest::Client::new();

        let tasks: serde_json::Value = client
            .get(format!("http://{}/api/v1/tasks", addrs.http))
            .send()
            .await
            .expect("tasks request")
            .json()
            .await
            .expect("tasks json");
        let task_id = tasks["tasks"][0]["taskId"]
            .as_str()
            .expect("task id")
            .to_string();

        let detail: serde_json::Value = client
            .get(format!("http://{}/api/v1/tasks/{}", addrs.http, task_id))
            .send()
            .await
            .expect("detail request")
            .json()
            .await
            .expect("detail json");
        assert_eq!(detail["taskId"], task_id);
        assert!(detail.get("gid").is_none());

        let paused: serde_json::Value = client
            .post(format!(
                "http://{}/api/v1/tasks/{}/pause",
                addrs.http, task_id
            ))
            .send()
            .await
            .expect("pause request")
            .json()
            .await
            .expect("pause json");
        assert_eq!(paused["lifecycle"], "paused");

        let resumed: serde_json::Value = client
            .post(format!(
                "http://{}/api/v1/tasks/{}/resume",
                addrs.http, task_id
            ))
            .send()
            .await
            .expect("resume request")
            .json()
            .await
            .expect("resume json");
        assert_eq!(resumed["lifecycle"], "queued");

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_detail_resolves_native_task_index_ids() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let task_id = TaskId::new();
        assert!(engine.register_native_task_id_for_migration(task_id.clone(), handle.gid));

        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let detail: serde_json::Value =
            reqwest::get(format!("http://{}/api/v1/tasks/{}", addrs.http, task_id))
                .await
                .expect("detail request")
                .json()
                .await
                .expect("detail json");

        assert_eq!(detail["taskId"], task_id.as_str());
        assert!(detail.get("gid").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn tasks_endpoint_projects_native_task_index_ids() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let task_id = TaskId::new();
        assert!(engine.register_native_task_id_for_migration(task_id.clone(), handle.gid));

        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let body: serde_json::Value = reqwest::get(format!("http://{}/api/v1/tasks", addrs.http))
            .await
            .expect("tasks request")
            .json()
            .await
            .expect("tasks json");

        assert_eq!(body["tasks"][0]["taskId"], task_id.as_str());
        assert!(body["tasks"][0].get("gid").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_files_and_sources_are_native_resources() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");
        let client = reqwest::Client::new();

        let created: serde_json::Value = client
            .post(format!("http://{}/api/v1/tasks", addrs.http))
            .json(&serde_json::json!({
                "sources": ["https://example.com/file.iso"],
                "downloadDir": "/tmp",
                "filename": "file.iso",
                "segments": 4
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");

        let task_id = created["taskId"].as_str().expect("task id");
        assert!(task_id.starts_with("task_"));
        assert!(!task_id.starts_with("task_migration_"));
        assert_eq!(created["lifecycle"], "queued");
        assert!(created.get("gid").is_none());

        let files: serde_json::Value = client
            .get(format!(
                "http://{}/api/v1/tasks/{}/files",
                addrs.http, task_id
            ))
            .send()
            .await
            .expect("files request")
            .json()
            .await
            .expect("files json");
        assert_eq!(files["files"][0]["path"], "/tmp/file.iso");
        assert!(files["files"][0].get("gid").is_none());

        let sources: serde_json::Value = client
            .get(format!(
                "http://{}/api/v1/tasks/{}/sources",
                addrs.http, task_id
            ))
            .send()
            .await
            .expect("sources request")
            .json()
            .await
            .expect("sources json");
        assert_eq!(sources["sources"][0]["protocol"], "https");
        assert!(sources["sources"][0].get("uri").is_some());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_event_uses_created_native_task_id() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let ws_url = format!("ws://{}/api/v1/events", addrs.http);
        let (mut events, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .expect("connect native events");
        let client = reqwest::Client::new();
        let created: serde_json::Value = client
            .post(format!("http://{}/api/v1/tasks", addrs.http))
            .json(&serde_json::json!({
                "sources": ["https://example.com/file.iso"],
                "downloadDir": "/tmp",
                "filename": "file.iso",
                "segments": 4
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");
        let task_id = created["taskId"].as_str().expect("task id");

        let event = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let frame = events
                    .next()
                    .await
                    .expect("event stream ended")
                    .expect("event frame");
                let json: serde_json::Value =
                    serde_json::from_str(frame.to_text().expect("event text")).expect("event json");
                if json["type"] == "task.started" {
                    break json;
                }
            }
        })
        .await
        .expect("timed out waiting for task started event");

        assert_eq!(event["taskId"], task_id);
        assert!(
            !event["taskId"]
                .as_str()
                .expect("event task id")
                .starts_with("task_migration_")
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn stats_endpoint_returns_native_global_counts() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add task");

        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let body: serde_json::Value = reqwest::get(format!("http://{}/api/v1/stats", addrs.http))
            .await
            .expect("stats request")
            .json()
            .await
            .expect("stats json");

        assert_eq!(body["taskCounts"]["queued"], 1);
        assert_eq!(body["downloadBytesPerSecond"], 0);
        assert!(body.get("numActive").is_none());

        cancel.cancel();
    }

    #[test]
    fn native_event_serializes_stable_type_string_and_task_id() {
        use raria_core::native::{NativeEvent, NativeEventData, NativeEventType, TaskId};

        let task_id = TaskId::new();
        let event = NativeEvent::new(
            7,
            NativeEventType::TaskProgress,
            Some(task_id.clone()),
            NativeEventData::Progress {
                completed_bytes: 10,
                total_bytes: Some(20),
                download_bytes_per_second: 5,
            },
        );

        let json = serde_json::to_value(event).expect("event json");

        assert_eq!(json["version"], 1);
        assert_eq!(json["sequence"], 7);
        assert_eq!(json["type"], "task.progress");
        assert_eq!(json["taskId"], task_id.as_str());
        assert_eq!(json["data"]["completedBytes"], 10);
        assert!(json.get("jsonrpc").is_none());
    }

    #[tokio::test]
    async fn native_events_websocket_streams_raria_event_envelopes() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let ws_url = format!("ws://{}/api/v1/events", addrs.http);
        let (mut ws, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .expect("connect native events");

        engine.event_bus.publish(DownloadEvent::Progress {
            gid: Gid::from_raw(9),
            downloaded: 128,
            total: Some(256),
            speed: 64,
        });

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .expect("event timeout")
            .expect("event frame")
            .expect("valid websocket frame");
        let text = msg.into_text().expect("text frame");
        let json: serde_json::Value = serde_json::from_str(&text).expect("event json");

        assert_eq!(json["type"], "task.progress");
        assert_eq!(json["data"]["completedBytes"], 128);
        assert!(json.get("jsonrpc").is_none());
        assert!(json.get("method").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_remove_and_restart_are_native_actions() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add task");

        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");
        let client = reqwest::Client::new();

        let tasks: serde_json::Value = client
            .get(format!("http://{}/api/v1/tasks", addrs.http))
            .send()
            .await
            .expect("tasks request")
            .json()
            .await
            .expect("tasks json");
        let task_id = tasks["tasks"][0]["taskId"].as_str().expect("task id");

        let removed: serde_json::Value = client
            .delete(format!("http://{}/api/v1/tasks/{}", addrs.http, task_id))
            .send()
            .await
            .expect("remove request")
            .json()
            .await
            .expect("remove json");
        assert_eq!(removed["lifecycle"], "removed");
        assert!(removed.get("gid").is_none());

        let restarted: serde_json::Value = client
            .post(format!(
                "http://{}/api/v1/tasks/{}/restart",
                addrs.http, task_id
            ))
            .send()
            .await
            .expect("restart request")
            .json()
            .await
            .expect("restart json");
        assert_eq!(restarted["lifecycle"], "queued");

        cancel.cancel();
    }

    #[tokio::test]
    async fn native_api_uses_bearer_token_auth_when_configured() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                auth_token: Some("secret-token".into()),
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");
        let client = reqwest::Client::new();

        let unauthenticated = client
            .get(format!("http://{}/api/v1/tasks", addrs.http))
            .send()
            .await
            .expect("unauthenticated request");
        assert_eq!(unauthenticated.status(), reqwest::StatusCode::UNAUTHORIZED);
        let error: serde_json::Value = unauthenticated.json().await.expect("error json");
        assert_eq!(error["code"], "auth_required");

        let unauthenticated_stats = client
            .get(format!("http://{}/api/v1/stats", addrs.http))
            .send()
            .await
            .expect("unauthenticated stats request");
        assert_eq!(
            unauthenticated_stats.status(),
            reqwest::StatusCode::UNAUTHORIZED
        );

        let unauthenticated_session_save = client
            .post(format!("http://{}/api/v1/session/save", addrs.http))
            .send()
            .await
            .expect("unauthenticated session save request");
        assert_eq!(
            unauthenticated_session_save.status(),
            reqwest::StatusCode::UNAUTHORIZED
        );

        let authenticated = client
            .get(format!("http://{}/api/v1/tasks", addrs.http))
            .bearer_auth("secret-token")
            .send()
            .await
            .expect("authenticated request");
        assert!(authenticated.status().is_success());

        cancel.cancel();
    }

    #[tokio::test]
    async fn config_endpoint_returns_native_runtime_projection() {
        let engine = Arc::new(Engine::new(GlobalConfig {
            max_concurrent_downloads: 12,
            split: 6,
            min_split_size: 1024,
            max_tries: 3,
            ..GlobalConfig::default()
        }));
        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");

        let body: serde_json::Value = reqwest::get(format!("http://{}/api/v1/config", addrs.http))
            .await
            .expect("config request")
            .json()
            .await
            .expect("config json");

        assert_eq!(body["daemon"]["maxActiveTasks"], 12);
        assert_eq!(body["downloads"]["defaultSegments"], 6);
        assert_eq!(body["downloads"]["minSegmentSize"], 1024);
        assert_eq!(body["downloads"]["retryMaxAttempts"], 3);
        assert!(body.get("rpcSecret").is_none());
        assert!(body.get("rpc_listen_port").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn session_save_endpoint_reports_native_store_status() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store_path = temp.path().join("native-session.redb");
        let store = Arc::new(raria_core::persist::Store::open(&store_path).expect("store"));
        let engine = Arc::new(Engine::with_store(
            GlobalConfig {
                session_file: store_path.clone(),
                ..GlobalConfig::default()
            },
            store,
        ));
        engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: temp.path().to_path_buf(),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add task");

        let cancel = CancellationToken::new();
        let addrs = start_native_api_server(
            Arc::clone(&engine),
            &NativeApiConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..NativeApiConfig::default()
            },
            cancel.clone(),
        )
        .await
        .expect("start native api");
        let client = reqwest::Client::new();

        let response: serde_json::Value = client
            .post(format!("http://{}/api/v1/session/save", addrs.http))
            .send()
            .await
            .expect("save session request")
            .json()
            .await
            .expect("save session json");

        assert_eq!(response["status"], "saved");
        assert_eq!(response["taskCount"], 1);
        assert_eq!(
            response["sessionPath"].as_str(),
            Some(store_path.to_str().expect("session path utf8"))
        );
        assert!(response.get("jsonrpc").is_none());
        assert!(store_path.is_file());

        cancel.cancel();
    }
}
