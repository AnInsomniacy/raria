#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::AddUriSpec;
    use raria_core::engine::Engine;
    use raria_core::job::Status;
    use raria_rpc::server::{RpcServerConfig, start_rpc_server};
    use std::net::SocketAddr;
    use std::path::PathBuf;
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
        assert!(
            body["uptime_seconds"].is_number(),
            "uptime must be a number"
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn health_endpoint_reports_correct_job_counts() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let waiting = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/waiting.iso".into()],
                filename: None,
                dir: PathBuf::from("/tmp"),
                connections: 1,
            })
            .unwrap();
        let seeding = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/seeding.iso".into()],
                filename: None,
                dir: PathBuf::from("/tmp"),
                connections: 1,
            })
            .unwrap();
        engine.activate_job(seeding.gid).unwrap();
        engine
            .registry
            .update(seeding.gid, |job| job.status = Status::Seeding)
            .unwrap();

        let cancel = CancellationToken::new();
        let config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(Arc::clone(&engine), &config, cancel.clone())
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let body: serde_json::Value = client
            .get(format!("http://127.0.0.1:{}/health", addrs.rpc.port()))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let waiting_gid = waiting.gid.to_string();
        assert_eq!(body["num_active"], 1, "seeding job should count as active");
        assert_eq!(
            body["num_waiting"], 1,
            "waiting job {waiting_gid} should stay waiting"
        );
        assert_eq!(body["num_stopped"], 0);

        cancel.cancel();
    }
}
