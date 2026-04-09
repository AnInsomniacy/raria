// Integration tests for WebSocket push notifications.
//
// Verifies that the RPC server sends aria2-compatible event notifications
// to connected WebSocket clients when download events occur.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{start_rpc_server, RpcServerConfig};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    /// A WS client should receive an aria2.onDownloadStart notification
    /// when a download is added via addUri.
    #[tokio::test]
    async fn ws_receives_on_download_start() {
        use futures::StreamExt;
        use tokio_tungstenite::connect_async;

        let config = GlobalConfig::default();
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();

        // Connect to the WS notification endpoint.
        let ws_url = format!("ws://{}", addrs.ws_notify);
        let (mut ws_stream, _) = connect_async(&ws_url)
            .await
            .expect("WS connect failed");

        // Give the WS connection a moment to establish.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Add a download via the engine directly (triggers event bus).
        let spec = raria_core::engine::AddUriSpec {
            uris: vec!["https://example.com/test.zip".into()],
            filename: Some("test.zip".into()),
            dir: std::path::PathBuf::from("/tmp"),
            connections: 1,
        };
        let _handle = engine.add_uri(&spec).unwrap();

        // Expect a notification within 2 seconds.
        let notification = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            ws_stream.next(),
        )
        .await;

        assert!(
            notification.is_ok(),
            "should receive WS notification within timeout"
        );

        let msg = notification.unwrap().unwrap().unwrap();
        let text = msg.to_text().unwrap();
        let json: serde_json::Value = serde_json::from_str(text).unwrap();

        // Verify aria2 notification format.
        assert_eq!(json["method"], "aria2.onDownloadStart");
        let params = json["params"].as_array().unwrap();
        assert!(!params.is_empty());
        assert!(params[0].get("gid").is_some());

        cancel.cancel();
    }

    /// WS notification should have the correct JSON-RPC 2.0 format.
    #[tokio::test]
    async fn ws_notification_format_is_jsonrpc() {
        use futures::StreamExt;
        use tokio_tungstenite::connect_async;

        let config = GlobalConfig::default();
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();

        let ws_url = format!("ws://{}", addrs.ws_notify);
        let (mut ws_stream, _) = connect_async(&ws_url)
            .await
            .expect("WS connect failed");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let spec = raria_core::engine::AddUriSpec {
            uris: vec!["https://example.com/notify_test.zip".into()],
            filename: None,
            dir: std::path::PathBuf::from("/tmp"),
            connections: 1,
        };
        let _handle = engine.add_uri(&spec).unwrap();

        let notification = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            ws_stream.next(),
        )
        .await;

        if let Ok(Some(Ok(msg))) = notification {
            let json: serde_json::Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
            // Must have jsonrpc field.
            assert_eq!(json["jsonrpc"], "2.0");
            // Must NOT have an id (notifications are one-way).
            assert!(json.get("id").is_none() || json["id"].is_null());
            // Must have method.
            assert!(json.get("method").is_some());
        } else {
            panic!("should have received notification");
        }

        cancel.cancel();
    }
}
