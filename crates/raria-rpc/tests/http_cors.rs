#[cfg(test)]
mod tests {
    use reqwest::header::{ACCESS_CONTROL_REQUEST_METHOD, ORIGIN};
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{start_rpc_server, RpcServerConfig};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn default_http_rpc_does_not_emit_cors_headers() {
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
            .post(format!("http://127.0.0.1:{}/jsonrpc", addrs.rpc.port()))
            .header(ORIGIN, "https://ui.example")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "aria2.getVersion",
                "params": [],
            }))
            .send()
            .await
            .unwrap();

        assert!(resp.status().is_success());
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "default server should not emit permissive CORS headers without explicit opt-in"
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn rpc_allow_origin_all_enables_jsonrpc_preflight_and_post_headers() {
        let engine = Arc::new(Engine::new(GlobalConfig {
            rpc_allow_origin_all: true,
            ..GlobalConfig::default()
        }));
        let cancel = CancellationToken::new();
        let config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine, &config, cancel.clone())
            .await
            .unwrap();

        let client = reqwest::Client::new();

        let preflight = client
            .request(
                reqwest::Method::OPTIONS,
                format!("http://127.0.0.1:{}/jsonrpc", addrs.rpc.port()),
            )
            .header(ORIGIN, "https://ui.example")
            .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .send()
            .await
            .unwrap();

        assert!(
            preflight.status().is_success(),
            "preflight should succeed when rpc_allow_origin_all is enabled: {}",
            preflight.status()
        );
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("*"),
        );

        let post = client
            .post(format!("http://127.0.0.1:{}/jsonrpc", addrs.rpc.port()))
            .header(ORIGIN, "https://ui.example")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "aria2.getVersion",
                "params": [],
            }))
            .send()
            .await
            .unwrap();

        assert!(post.status().is_success());
        assert_eq!(
            post.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("*"),
        );

        cancel.cancel();
    }
}
