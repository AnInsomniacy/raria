#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{RpcServerConfig, start_rpc_server};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn health_endpoint_returns_200_with_json() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine, &config, cancel.clone())
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{}/health", addrs.rpc.port()))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
        assert!(body["version"].is_string(), "version must be present");
        assert!(body["uptime_seconds"].is_number(), "uptime must be a number");

        cancel.cancel();
    }

    #[tokio::test]
    async fn health_endpoint_reports_correct_job_counts() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        // Add a job to the engine so we can verify counts are non-trivially zero.
        let addrs = {
            let cancel = CancellationToken::new();
            let config = RpcServerConfig {
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            };
            start_rpc_server(Arc::clone(&engine), &config, cancel.clone())
                .await
                .unwrap()
        };

        let client = reqwest::Client::new();
        let body: serde_json::Value = client
            .get(format!("http://127.0.0.1:{}/health", addrs.rpc.port()))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        // No jobs added — all counters should be 0.
        assert_eq!(body["num_active"], 0);
        assert_eq!(body["num_waiting"], 0);
    }
}
