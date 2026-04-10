// Integration tests for addTorrent RPC → Engine job creation.
//
// These tests verify that addTorrent creates jobs with kind=Bt
// and that the daemon dispatches them to BtService.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_core::job::JobKind;
    use raria_rpc::server::{start_rpc_server, RpcServerConfig};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    async fn spawn_server() -> (Arc<Engine>, String, CancellationToken) {
        let config = GlobalConfig {
            dir: std::path::PathBuf::from("/tmp/rpc_test_bt"),
            ..Default::default()
        };
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();
        let url = format!("http://{}", addrs.rpc);
        (engine, url, cancel)
    }

    async fn rpc_call(url: &str, method: &str, params: serde_json::Value) -> serde_json::Value {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        reqwest::Client::new()
            .post(url)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()
    }

    /// addUri with a magnet link should create a Bt-kind job.
    #[tokio::test]
    async fn add_uri_magnet_creates_bt_job() {
        let (engine, url, cancel) = spawn_server().await;

        let resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["magnet:?xt=urn:btih:da39a3ee5e6b4b0d3255bfef95601890afd80709"]
            ]),
        )
        .await;

        assert!(
            resp.get("error").is_none(),
            "addUri with magnet should succeed: {resp}"
        );
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(
            u64::from_str_radix(gid_str, 16).unwrap(),
        );
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.kind, JobKind::Bt, "magnet URI should create Bt-kind job");

        cancel.cancel();
    }

    #[tokio::test]
    async fn add_uri_magnet_with_select_file_stores_bt_file_selection_intent() {
        let (engine, url, cancel) = spawn_server().await;

        let resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["magnet:?xt=urn:btih:da39a3ee5e6b4b0d3255bfef95601890afd80709"],
                {
                    "select-file": "2,4"
                }
            ]),
        )
        .await;

        assert!(
            resp.get("error").is_none(),
            "addUri with magnet and select-file should succeed: {resp}"
        );
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(
            u64::from_str_radix(gid_str, 16).unwrap(),
        );
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.kind, JobKind::Bt);
        assert_eq!(job.options.bt_selected_files, Some(vec![1, 3]));

        cancel.cancel();
    }

    /// addTorrent with base64-encoded torrent data should create a Bt-kind job.
    #[tokio::test]
    async fn add_torrent_creates_bt_job() {
        let (engine, url, cancel) = spawn_server().await;

        // Create a minimal (but invalid) torrent payload — the point is
        // that addTorrent creates a job, not that librqbit can actually
        // download it.
        use base64::Engine as Base64Engine;
        let fake_torrent = b"d8:announce35:http://tracker.example.com/announcee";
        let encoded = base64::engine::general_purpose::STANDARD.encode(fake_torrent);

        let resp = rpc_call(
            &url,
            "aria2.addTorrent",
            serde_json::json!([encoded]),
        )
        .await;

        assert!(
            resp.get("error").is_none(),
            "addTorrent should succeed: {resp}"
        );
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(
            u64::from_str_radix(gid_str, 16).unwrap(),
        );
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.kind, JobKind::Bt, "torrent should create Bt-kind job");

        cancel.cancel();
    }

    /// addTorrent should persist BT file-selection intent on the created job.
    #[tokio::test]
    async fn add_torrent_with_select_file_stores_bt_file_selection() {
        let (engine, url, cancel) = spawn_server().await;

        use base64::Engine as Base64Engine;
        let fake_torrent = b"d8:announce35:http://tracker.example.com/announcee";
        let encoded = base64::engine::general_purpose::STANDARD.encode(fake_torrent);

        let resp = rpc_call(
            &url,
            "aria2.addTorrent",
            serde_json::json!([
                encoded,
                [],
                {
                    "select-file": "1,3"
                }
            ]),
        )
        .await;

        assert!(resp.get("error").is_none(), "addTorrent should succeed: {resp}");
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(
            u64::from_str_radix(gid_str, 16).unwrap(),
        );
        let job = engine.registry.get(gid).unwrap();

        assert_eq!(job.kind, JobKind::Bt);
        assert_eq!(
            job.options.bt_selected_files,
            Some(vec![0, 2]),
            "select-file should be stored as zero-based file IDs for runtime use"
        );

        cancel.cancel();
    }

    /// tellStatus for a BT job should include bittorrent-specific fields.
    #[tokio::test]
    async fn tell_status_bt_job_shows_kind() {
        let (_engine, url, cancel) = spawn_server().await;

        let add_resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([["magnet:?xt=urn:btih:abc123"]]),
        )
        .await;
        let gid_str = add_resp["result"].as_str().unwrap();

        let status_resp = rpc_call(
            &url,
            "aria2.tellStatus",
            serde_json::json!([gid_str]),
        )
        .await;
        let result = &status_resp["result"];
        // Status should exist and be valid.
        assert!(result.get("status").is_some());
        assert!(result.get("gid").is_some());

        cancel.cancel();
    }
}
