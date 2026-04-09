// Tests for getGlobalStat real-time accuracy.
//
// Verifies that getGlobalStat returns correct active/waiting/stopped counts
// and speed aggregations matching the engine's actual state.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{start_rpc_server, RpcServerConfig};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    async fn spawn_server() -> (Arc<Engine>, String, CancellationToken) {
        let config = GlobalConfig::default();
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

    /// getGlobalStat with no jobs should return all zeros.
    #[tokio::test]
    async fn global_stat_empty_engine() {
        let (_engine, url, cancel) = spawn_server().await;

        let resp = rpc_call(&url, "aria2.getGlobalStat", serde_json::json!([])).await;
        let result = &resp["result"];

        assert_eq!(result["numActive"].as_str().unwrap(), "0");
        assert_eq!(result["numWaiting"].as_str().unwrap(), "0");
        assert_eq!(result["numStopped"].as_str().unwrap(), "0");

        cancel.cancel();
    }

    /// getGlobalStat should reflect added jobs.
    #[tokio::test]
    async fn global_stat_reflects_jobs() {
        let (engine, url, cancel) = spawn_server().await;

        // Add 3 jobs.
        for i in 0..3 {
            let spec = raria_core::engine::AddUriSpec {
                uris: vec![format!("https://example.com/file{i}.zip")],
                filename: None,
                dir: std::path::PathBuf::from("/tmp"),
                connections: 1,
            };
            engine.add_uri(&spec).unwrap();
        }

        let resp = rpc_call(&url, "aria2.getGlobalStat", serde_json::json!([])).await;
        let result = &resp["result"];

        // Jobs start as waiting (none activated since daemon loop isn't running).
        let num_waiting: u64 = result["numWaiting"].as_str().unwrap().parse().unwrap();
        assert!(num_waiting > 0, "should have waiting jobs: {result}");

        cancel.cancel();
    }

    /// getGlobalStat speed fields should be string-encoded numbers.
    #[tokio::test]
    async fn global_stat_speed_fields_are_strings() {
        let (_engine, url, cancel) = spawn_server().await;

        let resp = rpc_call(&url, "aria2.getGlobalStat", serde_json::json!([])).await;
        let result = &resp["result"];

        // aria2 returns speeds as string-encoded integers.
        assert!(result["downloadSpeed"].as_str().is_some(), "downloadSpeed should be string");
        assert!(result["uploadSpeed"].as_str().is_some(), "uploadSpeed should be string");

        cancel.cancel();
    }
}
