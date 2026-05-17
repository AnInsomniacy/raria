#[cfg(test)]
mod tests {
    use base64::Engine as Base64Engine;
    use futures::StreamExt;
    use raria_core::config::GlobalConfig;
    use raria_core::engine::{AddUriSpec, Engine};
    use raria_core::job::{BtFile, BtPeer, Gid};
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
    async fn daemon_shutdown_endpoint_uses_native_api_envelope() {
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

        let body: serde_json::Value = reqwest::Client::new()
            .post(format!("http://{}/api/v1/daemon/shutdown", addrs.http))
            .send()
            .await
            .expect("shutdown request")
            .json()
            .await
            .expect("shutdown json");

        assert_eq!(body["status"], "shuttingDown");
        assert!(body.get("jsonrpc").is_none());
        assert!(body.get("result").is_none());
        assert!(engine.shutdown_token().is_cancelled());

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
        assert_eq!(sources["sources"][0]["health"]["state"], "unknown");
        assert_eq!(sources["sources"][0]["health"]["failureCount"], 0);

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_accepts_native_bt_options() {
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
                "sources": [
                    "magnet:?xt=urn:btih:feedface",
                    "https://webseed.example/file.iso"
                ],
                "downloadDir": "/tmp",
                "filename": "fixture.iso",
                "bt": {
                    "selectedFileIds": ["file_0", "file_2"],
                    "trackerUris": [
                        "udp://tracker.example:6969/announce",
                        "https://tracker.example/announce"
                    ],
                    "webSeedUris": ["https://explicit-webseed.example/file.iso"],
                    "deleteUnselectedFilesOnCompletion": true,
                    "seeding": {
                        "targetRatio": 1.25,
                        "stopAfterMinutes": 30
                    }
                }
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");

        let task_id = created["taskId"].as_str().expect("task id");
        assert!(task_id.starts_with("task_"));
        assert!(created.get("gid").is_none());
        assert!(created.get("bt-tracker").is_none());

        let gid = engine
            .gid_for_task_id(&TaskId::parse(task_id).expect("valid task id"))
            .expect("runtime gid");
        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.options.bt_selected_files, Some(vec![0, 2]));
        assert_eq!(
            job.options.bt_trackers,
            Some(vec![
                "udp://tracker.example:6969/announce".to_string(),
                "https://tracker.example/announce".to_string()
            ])
        );
        assert_eq!(
            job.options.bt_web_seed_uris,
            Some(vec![
                "https://explicit-webseed.example/file.iso".to_string()
            ])
        );
        assert!(job.options.bt_delete_unselected_files_on_completion);
        assert_eq!(job.options.seed_ratio, Some(1.25));
        assert_eq!(job.options.seed_time, Some(30));

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_torrent_source_uses_bt_backend() {
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
                "sources": ["torrent:base64:ZDQ6aW5mb2Rl"],
                "downloadDir": "/tmp",
                "filename": "fixture.torrent"
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");

        assert_eq!(created["sources"][0]["protocol"], "torrent");
        let task_id =
            TaskId::parse(created["taskId"].as_str().expect("task id")).expect("valid task id");
        let gid = engine.gid_for_task_id(&task_id).expect("runtime gid");
        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.kind, raria_core::job::JobKind::Bt);

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_metalink_bytes_creates_native_tasks() {
        let engine = Arc::new(Engine::new(GlobalConfig {
            metalink_preferred_locations: vec!["us".into()],
            metalink_unique_protocols: true,
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
        let client = reqwest::Client::new();
        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="alpha.bin">
    <size>10</size>
    <url priority="1" location="de">https://de.example/alpha.bin</url>
    <url priority="2" location="us">https://us.example/alpha.bin</url>
    <url priority="3">ftp://ftp.example/alpha.bin</url>
  </file>
  <file name="beta.bin">
    <size>20</size>
    <url priority="1">https://cdn.example/beta.bin</url>
  </file>
</metalink>"#;

        let created: serde_json::Value = client
            .post(format!("http://{}/api/v1/tasks", addrs.http))
            .json(&serde_json::json!({
                "downloadDir": "/tmp",
                "metalink": {
                    "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
                }
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");

        assert!(created["tasks"].as_array().is_some());
        let tasks = created["tasks"].as_array().expect("tasks");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0]["files"][0]["path"], "/tmp/alpha.bin");
        assert_eq!(
            tasks[0]["sources"][0]["uri"],
            "https://us.example/alpha.bin"
        );
        assert_eq!(tasks[0]["sources"][1]["uri"], "ftp://ftp.example/alpha.bin");
        assert_eq!(tasks[1]["files"][0]["path"], "/tmp/beta.bin");
        assert!(created.get("gid").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_metalink_torrent_metaurl_creates_native_bt_task() {
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
        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="example.iso">
    <size>1024</size>
    <url priority="1">https://mirror.example/example.iso</url>
    <metaurl mediatype="torrent" priority="1" name="example.iso.torrent">https://meta.example/example.iso.torrent</metaurl>
  </file>
</metalink>"#;

        let created: serde_json::Value = client
            .post(format!("http://{}/api/v1/tasks", addrs.http))
            .json(&serde_json::json!({
                "downloadDir": "/tmp",
                "metalink": {
                    "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
                }
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");

        let tasks = created["tasks"].as_array().expect("tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["sources"][0]["protocol"], "torrent");
        assert_eq!(
            tasks[0]["sources"][0]["uri"],
            "https://meta.example/example.iso.torrent"
        );
        let task_id =
            TaskId::parse(tasks[0]["taskId"].as_str().expect("task id")).expect("valid task id");
        let gid = engine.gid_for_task_id(&task_id).expect("runtime gid");
        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.kind, raria_core::job::JobKind::Bt);
        assert_eq!(
            job.options.bt_web_seed_uris,
            Some(vec!["https://mirror.example/example.iso".to_string()])
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_metalink_preserves_checksums_and_expected_size() {
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
        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="piece.bin">
    <size>2048</size>
    <hash type="md5">D41D8CD98F00B204E9800998ECF8427E</hash>
    <hash type="sha-256">ABCDEF123456</hash>
    <pieces type="sha-256" length="1024">
      <hash>AA</hash>
      <hash>BB</hash>
    </pieces>
    <url priority="1">https://mirror.example/piece.bin</url>
  </file>
</metalink>"#;

        let created: serde_json::Value = client
            .post(format!("http://{}/api/v1/tasks", addrs.http))
            .json(&serde_json::json!({
                "downloadDir": "/tmp",
                "metalink": {
                    "bytesBase64": base64::engine::general_purpose::STANDARD.encode(metalink_xml)
                }
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");

        let task_id = TaskId::parse(created["tasks"][0]["taskId"].as_str().expect("task id"))
            .expect("valid task id");
        let gid = engine.gid_for_task_id(&task_id).expect("runtime gid");
        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.total_size, Some(2048));
        assert_eq!(
            job.options.checksum.as_deref(),
            Some("sha-256=abcdef123456")
        );
        let piece_checksum = job.piece_checksum.as_ref().expect("piece checksum");
        assert_eq!(piece_checksum.algo, "sha-256");
        assert_eq!(piece_checksum.length, 1024);
        assert_eq!(piece_checksum.hashes, vec!["aa", "bb"]);

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_metalink_invalid_payload_returns_native_error() {
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

        let response = client
            .post(format!("http://{}/api/v1/tasks", addrs.http))
            .json(&serde_json::json!({
                "downloadDir": "/tmp",
                "metalink": {
                    "bytesBase64": "not-valid-base64"
                }
            }))
            .send()
            .await
            .expect("create request");
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let error: serde_json::Value = response.json().await.expect("error json");
        assert_eq!(error["code"], "invalid_request");
        assert!(error.get("gid").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_creation_metalink_path_creates_native_tasks() {
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
        let metalink_file = tempfile::NamedTempFile::new().expect("metalink file");
        std::fs::write(
            metalink_file.path(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="from-path.bin">
    <size>16</size>
    <url priority="1">https://mirror.example/from-path.bin</url>
  </file>
</metalink>"#,
        )
        .expect("write metalink");
        let client = reqwest::Client::new();

        let created: serde_json::Value = client
            .post(format!("http://{}/api/v1/tasks", addrs.http))
            .json(&serde_json::json!({
                "downloadDir": "/tmp",
                "metalink": {
                    "path": metalink_file.path()
                }
            }))
            .send()
            .await
            .expect("create request")
            .json()
            .await
            .expect("create json");

        let tasks = created["tasks"].as_array().expect("tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["files"][0]["path"], "/tmp/from-path.bin");
        assert_eq!(
            tasks[0]["sources"][0]["uri"],
            "https://mirror.example/from-path.bin"
        );
        assert!(created.get("gid").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_peers_and_trackers_are_native_resources() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["magnet:?xt=urn:btih:feedface".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("fixture.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let gid = engine
            .gid_for_task_id(&summary.task_id)
            .expect("runtime gid");
        engine.registry.update(gid, |job| {
            job.bt_peers = Some(vec![BtPeer {
                addr: "203.0.113.7:6881".to_string(),
                ip: "203.0.113.7".to_string(),
                port: 6881,
                download_speed: 1024,
                upload_speed: 256,
                seeder: true,
            }]);
            job.bt.get_or_insert_with(Default::default).announce_list =
                Some(vec!["udp://tracker.example:6969/announce".to_string()]);
        });

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

        let peers: serde_json::Value = client
            .get(format!(
                "http://{}/api/v1/tasks/{}/peers",
                addrs.http, summary.task_id
            ))
            .send()
            .await
            .expect("peers request")
            .json()
            .await
            .expect("peers json");
        assert_eq!(peers["peers"][0]["id"], "peer_203.0.113.7_6881");
        assert_eq!(peers["peers"][0]["downloadBytesPerSecond"], 1024);
        assert!(peers["peers"][0]["seeder"].as_bool().unwrap());
        assert!(peers["peers"][0].get("peerId").is_none());

        let trackers: serde_json::Value = client
            .get(format!(
                "http://{}/api/v1/tasks/{}/trackers",
                addrs.http, summary.task_id
            ))
            .send()
            .await
            .expect("trackers request")
            .json()
            .await
            .expect("trackers json");
        assert_eq!(trackers["trackers"][0]["id"], "tracker_0");
        assert_eq!(
            trackers["trackers"][0]["uri"],
            "udp://tracker.example:6969/announce"
        );
        assert!(trackers["trackers"][0].get("announce").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_trackers_patch_updates_native_bt_trackers() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["magnet:?xt=urn:btih:feedface".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("fixture.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let gid = engine
            .gid_for_task_id(&summary.task_id)
            .expect("runtime gid");

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

        let trackers: serde_json::Value = client
            .patch(format!(
                "http://{}/api/v1/tasks/{}/trackers",
                addrs.http, summary.task_id
            ))
            .json(&serde_json::json!({
                "trackerUris": [
                    "udp://tracker.example:6969/announce",
                    "https://tracker.example/announce"
                ],
                "excludedTrackerUris": [
                    "https://tracker.example/announce"
                ],
                "connectTimeoutSeconds": 3,
                "timeoutSeconds": 9,
                "intervalSeconds": 60
            }))
            .send()
            .await
            .expect("trackers patch request")
            .json()
            .await
            .expect("trackers patch json");

        assert_eq!(trackers["trackers"][0]["id"], "tracker_0");
        assert_eq!(
            trackers["trackers"][0]["uri"],
            "udp://tracker.example:6969/announce"
        );
        assert_eq!(trackers["trackers"][1]["id"], "tracker_1");
        assert_eq!(
            trackers["trackers"][1]["uri"],
            "https://tracker.example/announce"
        );
        assert_eq!(trackers["trackers"][0]["excluded"], false);
        assert_eq!(trackers["trackers"][1]["excluded"], true);
        assert_eq!(trackers["trackers"][0]["connectTimeoutSeconds"], 3);
        assert_eq!(trackers["trackers"][0]["timeoutSeconds"], 9);
        assert_eq!(trackers["trackers"][0]["intervalSeconds"], 60);
        assert!(trackers["trackers"][0].get("bt-tracker").is_none());

        let job = engine.registry.get(gid).expect("job");
        assert_eq!(
            job.options.bt_trackers,
            Some(vec![
                "udp://tracker.example:6969/announce".to_string(),
                "https://tracker.example/announce".to_string()
            ])
        );
        assert_eq!(
            job.options.bt_excluded_trackers,
            vec!["https://tracker.example/announce".to_string()]
        );
        assert_eq!(job.options.bt_tracker_connect_timeout_seconds, Some(3));
        assert_eq!(job.options.bt_tracker_timeout_seconds, Some(9));
        assert_eq!(job.options.bt_tracker_interval_seconds, Some(60));
        assert_eq!(
            job.bt.and_then(|bt| bt.announce_list),
            Some(vec![
                "udp://tracker.example:6969/announce".to_string(),
                "https://tracker.example/announce".to_string()
            ])
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_bt_seeding_patch_updates_native_seed_policy() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["magnet:?xt=urn:btih:feedface".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("fixture.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let gid = engine
            .gid_for_task_id(&summary.task_id)
            .expect("runtime gid");

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

        let policy: serde_json::Value = client
            .patch(format!(
                "http://{}/api/v1/tasks/{}/bt/seeding",
                addrs.http, summary.task_id
            ))
            .json(&serde_json::json!({
                "targetRatio": 1.5,
                "stopAfterMinutes": 45,
                "idleDownloadTimeoutSeconds": 7
            }))
            .send()
            .await
            .expect("seeding patch request")
            .json()
            .await
            .expect("seeding patch json");

        assert_eq!(policy["targetRatio"], 1.5);
        assert_eq!(policy["stopAfterMinutes"], 45);
        assert_eq!(policy["idleDownloadTimeoutSeconds"], 7);
        assert!(policy.get("seed-ratio").is_none());
        assert!(policy.get("bt-stop-timeout").is_none());

        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.options.seed_ratio, Some(1.5));
        assert_eq!(job.options.seed_time, Some(45));
        assert_eq!(job.options.bt_idle_download_timeout, Some(7));

        let readback: serde_json::Value = client
            .get(format!(
                "http://{}/api/v1/tasks/{}/bt/seeding",
                addrs.http, summary.task_id
            ))
            .send()
            .await
            .expect("seeding read request")
            .json()
            .await
            .expect("seeding read json");
        assert_eq!(readback["targetRatio"], 1.5);
        assert_eq!(readback["stopAfterMinutes"], 45);
        assert_eq!(readback["idleDownloadTimeoutSeconds"], 7);

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_transfer_patch_updates_native_runtime_limits() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("fixture.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let gid = engine
            .gid_for_task_id(&summary.task_id)
            .expect("runtime gid");

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

        let policy: serde_json::Value = client
            .patch(format!(
                "http://{}/api/v1/tasks/{}/transfer",
                addrs.http, summary.task_id
            ))
            .json(&serde_json::json!({
                "downloadBytesPerSecondLimit": 204800,
                "uploadBytesPerSecondLimit": 102400,
                "segments": 8
            }))
            .send()
            .await
            .expect("transfer patch request")
            .json()
            .await
            .expect("transfer patch json");

        assert_eq!(policy["downloadBytesPerSecondLimit"], 204800);
        assert_eq!(policy["uploadBytesPerSecondLimit"], 102400);
        assert_eq!(policy["segments"], 8);
        assert!(policy.get("max-download-limit").is_none());

        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.options.max_download_limit, 204800);
        assert_eq!(job.options.max_upload_limit, 102400);
        assert_eq!(job.options.max_connections, 8);
        assert_eq!(engine.job_rate_limiter(gid, 0).limit_bps(), 204800);

        let readback: serde_json::Value = client
            .get(format!(
                "http://{}/api/v1/tasks/{}/transfer",
                addrs.http, summary.task_id
            ))
            .send()
            .await
            .expect("transfer read request")
            .json()
            .await
            .expect("transfer read json");
        assert_eq!(readback["downloadBytesPerSecondLimit"], 204800);
        assert_eq!(readback["uploadBytesPerSecondLimit"], 102400);
        assert_eq!(readback["segments"], 8);

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_sources_patch_replaces_native_range_sources() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://old.example/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("fixture.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let gid = engine
            .gid_for_task_id(&summary.task_id)
            .expect("runtime gid");

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

        let sources: serde_json::Value = client
            .patch(format!(
                "http://{}/api/v1/tasks/{}/sources",
                addrs.http, summary.task_id
            ))
            .json(&serde_json::json!({
                "sources": [
                    "https://mirror-a.example/file.iso",
                    "https://mirror-b.example/file.iso"
                ]
            }))
            .send()
            .await
            .expect("sources patch request")
            .json()
            .await
            .expect("sources patch json");

        assert_eq!(sources["sources"][0]["protocol"], "https");
        assert_eq!(
            sources["sources"][0]["uri"],
            "https://mirror-a.example/file.iso"
        );
        assert_eq!(
            sources["sources"][1]["uri"],
            "https://mirror-b.example/file.iso"
        );
        assert!(sources.get("fileIndex").is_none());

        let job = engine.registry.get(gid).expect("job");
        assert_eq!(
            job.uris,
            vec![
                "https://mirror-a.example/file.iso".to_string(),
                "https://mirror-b.example/file.iso".to_string()
            ]
        );
        cancel.cancel();
    }

    #[tokio::test]
    async fn task_sources_get_projects_native_source_health() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec![
                    "https://slow.example/file.iso".into(),
                    "https://fast.example/file.iso".into(),
                ],
                dir: PathBuf::from("/tmp"),
                filename: Some("fixture.iso".into()),
                connections: 4,
            })
            .expect("add task");
        engine
            .source_failed_native_task(
                &summary.task_id,
                "https://slow.example/file.iso",
                "transient error: timeout",
            )
            .expect("record source failure");
        engine
            .source_succeeded_native_task(&summary.task_id, "https://fast.example/file.iso", 8192)
            .expect("record source success");

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

        let sources: serde_json::Value = client
            .get(format!(
                "http://{}/api/v1/tasks/{}/sources",
                addrs.http, summary.task_id
            ))
            .send()
            .await
            .expect("sources request")
            .json()
            .await
            .expect("sources json");

        assert_eq!(sources["sources"][0]["health"]["state"], "degraded");
        assert_eq!(sources["sources"][0]["health"]["failureCount"], 1);
        assert_eq!(
            sources["sources"][0]["health"]["lastError"],
            "transient error: timeout"
        );
        assert_eq!(sources["sources"][1]["health"]["state"], "healthy");
        assert_eq!(
            sources["sources"][1]["health"]["lastDownloadBytesPerSecond"],
            8192
        );
        assert!(
            sources["sources"][1]["health"]["score"]
                .as_u64()
                .expect("fast score")
                > sources["sources"][0]["health"]["score"]
                    .as_u64()
                    .expect("slow score")
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_queue_patch_updates_native_waiting_position() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let first = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://example.com/one.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("one.iso".into()),
                connections: 4,
            })
            .expect("add first");
        let second = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://example.com/two.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("two.iso".into()),
                connections: 4,
            })
            .expect("add second");
        let third = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://example.com/three.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("three.iso".into()),
                connections: 4,
            })
            .expect("add third");

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

        let queue: serde_json::Value = client
            .patch(format!(
                "http://{}/api/v1/tasks/{}/queue",
                addrs.http, third.task_id
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

        assert_eq!(queue["position"], 0);
        assert_eq!(queue["taskId"], third.task_id.as_str());
        assert!(queue.get("how").is_none());
        assert_eq!(
            engine.scheduler.waiting_task_queue(),
            vec![third.task_id, first.task_id, second.task_id]
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_files_patch_updates_native_bt_file_selection() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["magnet:?xt=urn:btih:feedface".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("fixture.iso".into()),
                connections: 4,
            })
            .expect("add task");
        let gid = engine
            .gid_for_task_id(&summary.task_id)
            .expect("runtime gid");
        engine.registry.update(gid, |job| {
            job.bt_files = Some(vec![
                BtFile {
                    index: 0,
                    path: PathBuf::from("disc/file-a.bin"),
                    length: 100,
                    completed_length: 25,
                    selected: true,
                },
                BtFile {
                    index: 1,
                    path: PathBuf::from("disc/file-b.bin"),
                    length: 200,
                    completed_length: 0,
                    selected: true,
                },
            ]);
        });

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

        let files: serde_json::Value = client
            .patch(format!(
                "http://{}/api/v1/tasks/{}/files",
                addrs.http, summary.task_id
            ))
            .json(&serde_json::json!({
                "selectedFileIds": ["file_1"]
            }))
            .send()
            .await
            .expect("files patch request")
            .json()
            .await
            .expect("files patch json");

        assert_eq!(files["files"][0]["id"], "file_0");
        assert_eq!(files["files"][0]["selected"], false);
        assert_eq!(files["files"][1]["id"], "file_1");
        assert_eq!(files["files"][1]["selected"], true);
        assert!(files["files"][0].get("index").is_none());

        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.options.bt_selected_files, Some(vec![1]));

        cancel.cancel();
    }

    #[tokio::test]
    async fn task_created_event_uses_created_native_task_id() {
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
                if json["type"] == "task.created" {
                    break json;
                }
            }
        })
        .await
        .expect("timed out waiting for task created event");

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

    #[tokio::test]
    async fn global_transfer_patch_updates_native_runtime_policy() {
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

        let policy: serde_json::Value = client
            .patch(format!("http://{}/api/v1/transfer", addrs.http))
            .json(&serde_json::json!({
                "downloadBytesPerSecondLimit": 4096,
                "uploadBytesPerSecondLimit": 2048,
                "maxActiveTasks": 7
            }))
            .send()
            .await
            .expect("global transfer patch request")
            .json()
            .await
            .expect("global transfer patch json");

        assert_eq!(policy["downloadBytesPerSecondLimit"], 4096);
        assert_eq!(policy["uploadBytesPerSecondLimit"], 2048);
        assert_eq!(policy["maxActiveTasks"], 7);
        assert!(policy.get("max-overall-download-limit").is_none());
        assert_eq!(engine.global_rate_limiter.limit_bps(), 4096);
        assert_eq!(engine.global_upload_limit_bps(), 2048);
        assert_eq!(engine.scheduler.max_concurrent(), 7);

        let readback: serde_json::Value = client
            .get(format!("http://{}/api/v1/transfer", addrs.http))
            .send()
            .await
            .expect("global transfer read request")
            .json()
            .await
            .expect("global transfer read json");
        assert_eq!(readback["downloadBytesPerSecondLimit"], 4096);
        assert_eq!(readback["uploadBytesPerSecondLimit"], 2048);
        assert_eq!(readback["maxActiveTasks"], 7);

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
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add native task");
        let runtime_gid = engine
            .gid_for_task_id(&summary.task_id)
            .expect("runtime gid");
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
            downloaded: 64,
            total: Some(256),
            speed: 32,
        });
        engine.event_bus.publish(DownloadEvent::Progress {
            gid: runtime_gid,
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
        assert_eq!(json["taskId"], summary.task_id.as_str());
        assert_eq!(json["data"]["completedBytes"], 128);
        assert!(json.get("jsonrpc").is_none());
        assert!(json.get("method").is_none());

        cancel.cancel();
    }

    #[tokio::test]
    async fn native_events_websocket_prefers_native_event_bus() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add native task");
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
            gid: Gid::from_raw(77),
            downloaded: 64,
            total: Some(256),
            speed: 32,
        });
        engine
            .source_failed_native_task(
                &summary.task_id,
                "https://mirror.example/file.iso",
                "transient error: timeout",
            )
            .expect("publish native source failure");

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .expect("event timeout")
            .expect("event frame")
            .expect("valid websocket frame");
        let text = msg.into_text().expect("text frame");
        let json: serde_json::Value = serde_json::from_str(&text).expect("event json");

        assert_eq!(json["type"], "task.source.failed");
        assert_eq!(json["taskId"], summary.task_id.as_str());
        assert_eq!(json["data"]["code"], "source_failed");
        assert_eq!(json["data"]["message"], "transient error: timeout");

        cancel.cancel();
    }

    #[tokio::test]
    async fn native_events_websocket_streams_native_lifecycle_events() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let summary = engine
            .add_native_task(&AddUriSpec {
                uris: vec!["https://example.com/file.iso".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("file.iso".into()),
                connections: 4,
            })
            .expect("add native task");
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

        engine.activate_native_task(&summary.task_id).unwrap();
        let started = next_native_event(&mut ws).await;
        assert_eq!(started["type"], "task.started");
        assert_eq!(started["taskId"], summary.task_id.as_str());

        engine.pause_native_task(&summary.task_id).unwrap();
        let paused = next_native_event(&mut ws).await;
        assert_eq!(paused["type"], "task.paused");
        assert_eq!(paused["taskId"], summary.task_id.as_str());

        engine.resume_native_task(&summary.task_id).unwrap();
        let resumed = next_native_event(&mut ws).await;
        assert_eq!(resumed["type"], "task.resumed");
        assert_eq!(resumed["taskId"], summary.task_id.as_str());

        engine.remove_native_task(&summary.task_id).unwrap();
        let removed = next_native_event(&mut ws).await;
        assert_eq!(removed["type"], "task.removed");
        assert_eq!(removed["taskId"], summary.task_id.as_str());

        cancel.cancel();
    }

    async fn next_native_event(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> serde_json::Value {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .expect("event timeout")
            .expect("event frame")
            .expect("valid websocket frame");
        let text = msg.into_text().expect("text frame");
        serde_json::from_str(&text).expect("event json")
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

        let unauthenticated_shutdown = client
            .post(format!("http://{}/api/v1/daemon/shutdown", addrs.http))
            .send()
            .await
            .expect("unauthenticated shutdown request");
        assert_eq!(
            unauthenticated_shutdown.status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        assert!(!engine.shutdown_token().is_cancelled());

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
            metalink_preferred_locations: vec!["us".into(), "jp".into()],
            metalink_preferred_protocol: Some("https".into()),
            metalink_unique_protocols: true,
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
        assert_eq!(
            body["metalink"]["preferredLocations"],
            serde_json::json!(["us", "jp"])
        );
        assert_eq!(body["metalink"]["preferredProtocol"], "https");
        assert_eq!(body["metalink"]["uniqueProtocols"], true);
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
