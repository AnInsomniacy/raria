// Integration tests for --rpc-secret token authentication.
//
// Verifies that when rpc_secret is configured, all RPC requests
// must include the secret token as the first parameter.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{start_rpc_server, RpcServerConfig};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

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

    /// When rpc_secret is set, requests without token should fail.
    #[tokio::test]
    async fn rpc_secret_rejects_unauthenticated() {
        let config = GlobalConfig {
            rpc_secret: Some("mysecret123".into()),
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

        // Request without token should fail.
        let resp = rpc_call(&url, "aria2.getVersion", serde_json::json!([])).await;
        assert!(
            resp.get("error").is_some(),
            "should reject request without token: {resp}"
        );

        cancel.cancel();
    }

    /// When rpc_secret is set, requests with correct token:prefix should succeed.
    #[tokio::test]
    async fn rpc_secret_accepts_valid_token() {
        let config = GlobalConfig {
            rpc_secret: Some("mysecret123".into()),
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

        // Request with correct token should succeed.
        // aria2 format: first param is "token:<secret>"
        let resp = rpc_call(
            &url,
            "aria2.getVersion",
            serde_json::json!(["token:mysecret123"]),
        )
        .await;
        assert!(
            resp.get("error").is_none(),
            "should accept valid token: {resp}"
        );
        assert!(resp["result"]["version"].is_string());

        cancel.cancel();
    }

    /// When rpc_secret is set, requests with wrong token should fail.
    #[tokio::test]
    async fn rpc_secret_rejects_wrong_token() {
        let config = GlobalConfig {
            rpc_secret: Some("correctsecret".into()),
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

        let resp = rpc_call(
            &url,
            "aria2.getVersion",
            serde_json::json!(["token:wrongsecret"]),
        )
        .await;
        assert!(
            resp.get("error").is_some(),
            "should reject wrong token: {resp}"
        );

        cancel.cancel();
    }

    /// When NO rpc_secret is configured, all requests should succeed.
    #[tokio::test]
    async fn no_rpc_secret_allows_all() {
        let config = GlobalConfig::default(); // No secret.
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();
        let url = format!("http://{}", addrs.rpc);

        let resp = rpc_call(&url, "aria2.getVersion", serde_json::json!([])).await;
        assert!(
            resp.get("error").is_none(),
            "should allow without secret: {resp}"
        );

        cancel.cancel();
    }
}
