// RPC method integration tests — options propagation.
//
// These tests verify that RPC method calls correctly propagate
// per-job options (headers, checksum, speed limits) to the Job
// and that changeOption/getOption work with real values.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{RpcServerConfig, start_rpc_server};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    async fn spawn_server() -> (Arc<Engine>, String, CancellationToken) {
        let config = GlobalConfig {
            dir: std::path::PathBuf::from("/tmp/rpc_test_options"),
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

    // ── addUri with header propagation ──────────────────────────────

    #[tokio::test]
    async fn add_uri_with_headers_stores_in_job_options() {
        let (engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["https://example.com/file.bin"],
                {
                    "header": ["Referer: https://origin.com", "X-Token: abc123"],
                }
            ]),
        )
        .await;
        assert!(resp.get("error").is_none(), "addUri should succeed: {resp}");
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.headers.len(), 2);
        assert_eq!(job.options.headers[0].0, "Referer");
        assert_eq!(job.options.headers[0].1, "https://origin.com");
        assert_eq!(job.options.headers[1].0, "X-Token");
        assert_eq!(job.options.headers[1].1, "abc123");
        cancel.cancel();
    }

    #[tokio::test]
    async fn add_uri_with_checksum_stores_in_job_options() {
        let (engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["https://example.com/f.zip"],
                { "checksum": "sha-256=abcdef1234567890" }
            ]),
        )
        .await;
        assert!(resp.get("error").is_none());
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(
            job.options.checksum,
            Some("sha-256=abcdef1234567890".into())
        );
        cancel.cancel();
    }

    #[tokio::test]
    async fn add_uri_with_max_download_limit_stores_in_job_options() {
        let (engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["https://example.com/f.zip"],
                { "max-download-limit": "102400" }
            ]),
        )
        .await;
        assert!(resp.get("error").is_none());
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.max_download_limit, 102400);
        cancel.cancel();
    }

    #[tokio::test]
    async fn add_uri_with_http_basic_auth_stores_in_job_options() {
        let (engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["https://example.com/protected.bin"],
                {
                    "http-user": "rpc-user",
                    "http-passwd": "rpc-pass"
                }
            ]),
        )
        .await;
        assert!(resp.get("error").is_none());
        let gid_str = resp["result"].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.http_user.as_deref(), Some("rpc-user"));
        assert_eq!(job.options.http_passwd.as_deref(), Some("rpc-pass"));
        cancel.cancel();
    }

    #[tokio::test]
    async fn get_option_round_trips_bt_tracker() {
        let (engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["magnet:?xt=urn:btih:da39a3ee5e6b4b0d3255bfef95601890afd80709"],
                {
                    "bt-tracker": "http://tracker1.example/announce,http://tracker2.example/announce"
                }
            ]),
        )
        .await;
        assert!(resp.get("error").is_none(), "addUri should succeed: {resp}");

        let gid = resp["result"].as_str().unwrap().to_string();
        let option = rpc_call(&url, "aria2.getOption", serde_json::json!([gid.clone()])).await;
        assert_eq!(
            option["result"]["bt-tracker"],
            "http://tracker1.example/announce,http://tracker2.example/announce"
        );

        let parsed_gid = raria_core::job::Gid::from_raw(u64::from_str_radix(&gid, 16).unwrap());
        let job = engine.registry.get(parsed_gid).unwrap();
        assert_eq!(
            job.options.bt_trackers,
            Some(vec![
                "http://tracker1.example/announce".into(),
                "http://tracker2.example/announce".into(),
            ])
        );

        cancel.cancel();
    }

    // ── changeOption ────────────────────────────────────────────────

    #[tokio::test]
    async fn change_option_updates_max_download_limit() {
        let (engine, url, cancel) = spawn_server().await;
        // Add a job first.
        let add_resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([["https://example.com/f.zip"]]),
        )
        .await;
        let gid_str = add_resp["result"].as_str().unwrap();

        // Change the limit.
        let change_resp = rpc_call(
            &url,
            "aria2.changeOption",
            serde_json::json!([gid_str, {"max-download-limit": "204800"}]),
        )
        .await;
        assert_eq!(change_resp["result"], "OK");

        // Verify through engine.
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.max_download_limit, 204800);
        cancel.cancel();
    }

    #[tokio::test]
    async fn change_option_updates_split() {
        let (engine, url, cancel) = spawn_server().await;
        let add_resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([["https://example.com/f.zip"]]),
        )
        .await;
        let gid_str = add_resp["result"].as_str().unwrap();

        let change_resp = rpc_call(
            &url,
            "aria2.changeOption",
            serde_json::json!([gid_str, {"split": "4"}]),
        )
        .await;
        assert_eq!(change_resp["result"], "OK");

        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.max_connections, 4);
        cancel.cancel();
    }

    // ── getOption returns real values ────────────────────────────────

    #[tokio::test]
    async fn get_option_returns_real_job_options() {
        let (_engine, url, cancel) = spawn_server().await;
        // Add with custom options.
        let add_resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["https://example.com/f.zip"],
                {
                    "max-download-limit": "51200",
                    "header": ["Referer: https://test.com"],
                    "checksum": "sha-256=aabbccdd"
                }
            ]),
        )
        .await;
        let gid_str = add_resp["result"].as_str().unwrap();

        // Get option.
        let opt_resp = rpc_call(&url, "aria2.getOption", serde_json::json!([gid_str])).await;
        let result = &opt_resp["result"];
        assert_eq!(result["max-download-limit"], "51200");
        assert_eq!(result["checksum"], "sha-256=aabbccdd");
        // header should be an array with our value.
        let headers = result["header"].as_array().unwrap();
        assert!(
            headers
                .iter()
                .any(|h| h.as_str() == Some("Referer: https://test.com"))
        );
        cancel.cancel();
    }

    #[tokio::test]
    async fn get_option_returns_http_basic_auth_fields() {
        let (_engine, url, cancel) = spawn_server().await;
        let add_resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([
                ["https://example.com/private.bin"],
                {
                    "http-user": "rpc-user",
                    "http-passwd": "rpc-pass"
                }
            ]),
        )
        .await;
        let gid_str = add_resp["result"].as_str().unwrap();

        let opt_resp = rpc_call(&url, "aria2.getOption", serde_json::json!([gid_str])).await;
        let result = &opt_resp["result"];
        assert_eq!(result["http-user"], "rpc-user");
        assert_eq!(result["http-passwd"], "rpc-pass");
        cancel.cancel();
    }

    #[tokio::test]
    async fn get_option_round_trips_bt_select_file_as_aria2_string() {
        use base64::Engine as Base64Engine;

        let (_engine, url, cancel) = spawn_server().await;
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(b"d8:announce35:http://tracker.example.com/announcee");

        let add_resp = rpc_call(
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
        assert!(
            add_resp.get("error").is_none(),
            "addTorrent should succeed: {add_resp}"
        );
        let gid_str = add_resp["result"].as_str().unwrap();

        let option = rpc_call(&url, "aria2.getOption", serde_json::json!([gid_str])).await;
        assert_eq!(option["result"]["select-file"], "1,3");
        cancel.cancel();
    }

    #[tokio::test]
    async fn get_option_round_trips_bt_seed_controls() {
        use base64::Engine as Base64Engine;

        let (engine, url, cancel) = spawn_server().await;
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(b"d8:announce35:http://tracker.example.com/announcee");

        let add_resp = rpc_call(
            &url,
            "aria2.addTorrent",
            serde_json::json!([
                encoded,
                [],
                {
                    "seed-ratio": "1.5",
                    "seed-time": "60"
                }
            ]),
        )
        .await;
        assert!(
            add_resp.get("error").is_none(),
            "addTorrent should succeed: {add_resp}"
        );
        let gid_str = add_resp["result"].as_str().unwrap();

        let option = rpc_call(&url, "aria2.getOption", serde_json::json!([gid_str])).await;
        assert_eq!(option["result"]["seed-ratio"], "1.5");
        assert_eq!(option["result"]["seed-time"], "60");

        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.seed_ratio, Some(1.5));
        assert_eq!(job.options.seed_time, Some(60));

        cancel.cancel();
    }

    #[tokio::test]
    async fn change_option_updates_bt_seed_controls_and_trackers() {
        use base64::Engine as Base64Engine;

        let (engine, url, cancel) = spawn_server().await;
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(b"d8:announce35:http://tracker.example.com/announcee");

        let add_resp = rpc_call(
            &url,
            "aria2.addTorrent",
            serde_json::json!([encoded, [], {}]),
        )
        .await;
        assert!(
            add_resp.get("error").is_none(),
            "addTorrent should succeed: {add_resp}"
        );
        let gid_str = add_resp["result"].as_str().unwrap().to_string();

        let change_resp = rpc_call(
            &url,
            "aria2.changeOption",
            serde_json::json!([
                gid_str.clone(),
                {
                    "bt-tracker": "http://tracker3.example/announce",
                    "seed-ratio": "2",
                    "seed-time": "30"
                }
            ]),
        )
        .await;
        assert_eq!(change_resp["result"], "OK");

        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(&gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(
            job.options.bt_trackers,
            Some(vec!["http://tracker3.example/announce".into()])
        );
        assert_eq!(job.options.seed_ratio, Some(2.0));
        assert_eq!(job.options.seed_time, Some(30));

        cancel.cancel();
    }

    #[tokio::test]
    async fn change_option_updates_bt_select_file_and_get_option_round_trips_it() {
        use base64::Engine as Base64Engine;

        let (engine, url, cancel) = spawn_server().await;
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(b"d8:announce35:http://tracker.example.com/announcee");

        let add_resp = rpc_call(&url, "aria2.addTorrent", serde_json::json!([encoded])).await;
        assert!(
            add_resp.get("error").is_none(),
            "addTorrent should succeed: {add_resp}"
        );
        let gid_str = add_resp["result"].as_str().unwrap().to_string();

        let change_resp = rpc_call(
            &url,
            "aria2.changeOption",
            serde_json::json!([
                gid_str.clone(),
                {"select-file": "2,4"}
            ]),
        )
        .await;
        assert_eq!(change_resp["result"], "OK");

        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(&gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.bt_selected_files, Some(vec![1, 3]));

        let option = rpc_call(&url, "aria2.getOption", serde_json::json!([gid_str])).await;
        assert_eq!(option["result"]["select-file"], "2,4");

        cancel.cancel();
    }

    #[tokio::test]
    async fn change_option_updates_checksum_and_http_auth_fields() {
        let (engine, url, cancel) = spawn_server().await;
        let add_resp = rpc_call(
            &url,
            "aria2.addUri",
            serde_json::json!([["https://example.com/f.zip"]]),
        )
        .await;
        let gid_str = add_resp["result"].as_str().unwrap().to_string();

        let change_resp = rpc_call(
            &url,
            "aria2.changeOption",
            serde_json::json!([
                gid_str.clone(),
                {
                    "checksum": "sha-256=abc123",
                    "http-user": "rpc-user",
                    "http-passwd": "rpc-pass"
                }
            ]),
        )
        .await;
        assert_eq!(change_resp["result"], "OK");

        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(&gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.options.checksum.as_deref(), Some("sha-256=abc123"));
        assert_eq!(job.options.http_user.as_deref(), Some("rpc-user"));
        assert_eq!(job.options.http_passwd.as_deref(), Some("rpc-pass"));

        let option = rpc_call(&url, "aria2.getOption", serde_json::json!([gid_str])).await;
        assert_eq!(option["result"]["checksum"], "sha-256=abc123");
        assert_eq!(option["result"]["http-user"], "rpc-user");
        assert_eq!(option["result"]["http-passwd"], "rpc-pass");

        cancel.cancel();
    }

    // ── getGlobalOption includes proxy/TLS fields ────────────────────

    #[tokio::test]
    async fn get_global_option_includes_proxy_fields() {
        let (_engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(&url, "aria2.getGlobalOption", serde_json::json!([])).await;
        let result = &resp["result"];

        // These fields must exist (even if empty).
        assert!(result.get("all-proxy").is_some());
        assert!(result.get("http-proxy").is_some());
        assert!(result.get("https-proxy").is_some());
        assert!(result.get("no-proxy").is_some());
        assert!(result.get("check-certificate").is_some());
        assert!(result.get("user-agent").is_some());
        cancel.cancel();
    }

    // ── changeGlobalOption ──────────────────────────────────────────

    #[tokio::test]
    async fn change_global_option_updates_concurrent() {
        let (engine, url, cancel) = spawn_server().await;
        assert_eq!(engine.scheduler.max_concurrent(), 5); // Default.

        let resp = rpc_call(
            &url,
            "aria2.changeGlobalOption",
            serde_json::json!([{"max-concurrent-downloads": "10"}]),
        )
        .await;
        assert_eq!(resp["result"], "OK");

        // Verify the scheduler was actually updated.
        assert_eq!(engine.scheduler.max_concurrent(), 10);
        cancel.cancel();
    }

    #[tokio::test]
    async fn change_global_option_updates_runtime_download_limit() {
        let (engine, url, cancel) = spawn_server().await;
        assert_eq!(engine.global_rate_limiter.limit_bps(), 0);

        let resp = rpc_call(
            &url,
            "aria2.changeGlobalOption",
            serde_json::json!([{"max-overall-download-limit": "2048"}]),
        )
        .await;
        assert_eq!(resp["result"], "OK");
        assert_eq!(engine.global_rate_limiter.limit_bps(), 2048);

        let resp = rpc_call(
            &url,
            "aria2.changeGlobalOption",
            serde_json::json!([{"max-overall-download-limit": "0"}]),
        )
        .await;
        assert_eq!(resp["result"], "OK");
        assert_eq!(engine.global_rate_limiter.limit_bps(), 0);

        cancel.cancel();
    }

    #[tokio::test]
    async fn change_global_option_accepts_max_download_limit_alias() {
        let (engine, url, cancel) = spawn_server().await;
        assert_eq!(engine.global_rate_limiter.limit_bps(), 0);

        let resp = rpc_call(
            &url,
            "aria2.changeGlobalOption",
            serde_json::json!([{"max-download-limit": "3072"}]),
        )
        .await;
        assert_eq!(resp["result"], "OK");
        assert_eq!(engine.global_rate_limiter.limit_bps(), 3072);

        cancel.cancel();
    }

    #[tokio::test]
    async fn get_global_option_reflects_runtime_download_limit_after_mutation() {
        let (_engine, url, cancel) = spawn_server().await;

        let resp = rpc_call(
            &url,
            "aria2.changeGlobalOption",
            serde_json::json!([{"max-overall-download-limit": "4096"}]),
        )
        .await;
        assert_eq!(resp["result"], "OK");

        let global = rpc_call(&url, "aria2.getGlobalOption", serde_json::json!([])).await;
        assert_eq!(global["result"]["max-overall-download-limit"], "4096");

        let resp = rpc_call(
            &url,
            "aria2.changeGlobalOption",
            serde_json::json!([{"max-overall-download-limit": "0"}]),
        )
        .await;
        assert_eq!(resp["result"], "OK");

        let global = rpc_call(&url, "aria2.getGlobalOption", serde_json::json!([])).await;
        assert_eq!(global["result"]["max-overall-download-limit"], "0");

        cancel.cancel();
    }

    #[tokio::test]
    async fn get_global_option_reflects_runtime_max_concurrent_after_mutation() {
        let (_engine, url, cancel) = spawn_server().await;

        let resp = rpc_call(
            &url,
            "aria2.changeGlobalOption",
            serde_json::json!([{"max-concurrent-downloads": "9"}]),
        )
        .await;
        assert_eq!(resp["result"], "OK");

        let global = rpc_call(&url, "aria2.getGlobalOption", serde_json::json!([])).await;
        assert_eq!(global["result"]["max-concurrent-downloads"], "9");

        cancel.cancel();
    }

    // ── getSessionInfo ──────────────────────────────────────────────

    #[tokio::test]
    async fn get_session_info_returns_hex_id() {
        let (_engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(&url, "aria2.getSessionInfo", serde_json::json!([])).await;
        let session_id = resp["result"]["sessionId"].as_str().unwrap();
        // Must be 16-char hex.
        assert_eq!(
            session_id.len(),
            16,
            "session ID must be 16 chars: {session_id}"
        );
        assert!(
            session_id.chars().all(|c| c.is_ascii_hexdigit()),
            "session ID must be hex: {session_id}"
        );
        cancel.cancel();
    }
}
