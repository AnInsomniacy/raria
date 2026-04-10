// RPC method integration tests — options propagation.
//
// These tests verify that RPC method calls correctly propagate
// per-job options (headers, checksum, speed limits) to the Job
// and that changeOption/getOption work with real values.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{start_rpc_server, RpcServerConfig};
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
        let resp = reqwest::Client::new()
            .post(url)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        resp
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
        assert_eq!(job.options.checksum, Some("sha-256=abcdef1234567890".into()));
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
        let opt_resp = rpc_call(
            &url,
            "aria2.getOption",
            serde_json::json!([gid_str]),
        )
        .await;
        let result = &opt_resp["result"];
        assert_eq!(result["max-download-limit"], "51200");
        assert_eq!(result["checksum"], "sha-256=aabbccdd");
        // header should be an array with our value.
        let headers = result["header"].as_array().unwrap();
        assert!(headers.iter().any(|h| h.as_str() == Some("Referer: https://test.com")));
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

    // ── getGlobalOption includes proxy/TLS fields ────────────────────

    #[tokio::test]
    async fn get_global_option_includes_proxy_fields() {
        let (_engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(
            &url,
            "aria2.getGlobalOption",
            serde_json::json!([]),
        )
        .await;
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

    // ── getSessionInfo ──────────────────────────────────────────────

    #[tokio::test]
    async fn get_session_info_returns_hex_id() {
        let (_engine, url, cancel) = spawn_server().await;
        let resp = rpc_call(
            &url,
            "aria2.getSessionInfo",
            serde_json::json!([]),
        )
        .await;
        let session_id = resp["result"]["sessionId"].as_str().unwrap();
        // Must be 16-char hex.
        assert_eq!(session_id.len(), 16, "session ID must be 16 chars: {session_id}");
        assert!(
            session_id.chars().all(|c| c.is_ascii_hexdigit()),
            "session ID must be hex: {session_id}"
        );
        cancel.cancel();
    }
}
