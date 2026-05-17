#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{RpcServerConfig, start_rpc_server};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    async fn spawn_server() -> (String, CancellationToken) {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine, &rpc_config, cancel.clone())
            .await
            .unwrap();
        (format!("http://{}", addrs.rpc), cancel)
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

    #[tokio::test]
    async fn legacy_add_metalink_is_not_registered() {
        let (url, cancel) = spawn_server().await;

        let response = rpc_call(&url, "aria2.addMetalink", serde_json::json!([""])).await;

        assert_eq!(response["error"]["code"], -32601);
        assert!(response.get("result").is_none());
        cancel.cancel();
    }

    #[tokio::test]
    async fn legacy_add_torrent_is_not_registered() {
        let (url, cancel) = spawn_server().await;

        let response = rpc_call(&url, "aria2.addTorrent", serde_json::json!([""])).await;

        assert_eq!(response["error"]["code"], -32601);
        assert!(response.get("result").is_none());
        cancel.cancel();
    }
}
