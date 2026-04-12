// Integration tests for WebSocket push notifications.
//
// Verifies that the RPC server sends aria2-compatible event notifications
// to connected WebSocket clients when download events occur.

#[cfg(test)]
mod tests {
    use futures::{SinkExt, StreamExt};
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{RpcServerConfig, start_rpc_server};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
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
        let (mut ws_stream, _) = connect_async(&ws_url).await.expect("WS connect failed");

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
        let notification =
            tokio::time::timeout(std::time::Duration::from_secs(2), ws_stream.next()).await;

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

    #[tokio::test]
    async fn ws_receives_on_source_failed() {
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
        let (mut ws_stream, _) = connect_async(&ws_url).await.expect("WS connect failed");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let spec = raria_core::engine::AddUriSpec {
            uris: vec!["https://example.com/source-failed.bin".into()],
            filename: Some("source-failed.bin".into()),
            dir: std::path::PathBuf::from("/tmp"),
            connections: 1,
        };
        let handle = engine.add_uri(&spec).unwrap();

        engine
            .source_failed(
                handle.gid,
                "https://mirror.example/file.iso",
                "permanent error: checksum mismatch",
            )
            .unwrap();

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let source_failed_json = loop {
            let msg = tokio::time::timeout_at(deadline, ws_stream.next())
                .await
                .expect("timed out waiting for WS source-failed notification")
                .expect("stream ended before source-failed notification")
                .expect("WS notification frame error");
            let json: serde_json::Value =
                serde_json::from_str(msg.to_text().expect("WS text frame")).unwrap();
            if json["method"] == "aria2.onSourceFailed" {
                break json;
            }
        };

        let params = source_failed_json["params"].as_array().unwrap();
        assert_eq!(params[0]["gid"], format!("{}", handle.gid));

        cancel.cancel();
    }

    /// WS notification should have the correct JSON-RPC 2.0 format.
    #[tokio::test]
    async fn ws_notification_format_is_jsonrpc() {
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
        let (mut ws_stream, _) = connect_async(&ws_url).await.expect("WS connect failed");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let spec = raria_core::engine::AddUriSpec {
            uris: vec!["https://example.com/notify_test.zip".into()],
            filename: None,
            dir: std::path::PathBuf::from("/tmp"),
            connections: 1,
        };
        let _handle = engine.add_uri(&spec).unwrap();

        let notification =
            tokio::time::timeout(std::time::Duration::from_secs(2), ws_stream.next()).await;

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

    /// aria2-compatible clients expect to use a single WebSocket connection
    /// at `/jsonrpc` for both JSON-RPC requests and unsolicited notifications.
    ///
    /// This test intentionally codifies that contract:
    /// 1. Establish one WS connection to the RPC endpoint
    /// 2. Send `aria2.addUri` over that WS connection
    /// 3. Observe both the RPC response and an `aria2.onDownloadStart`
    ///    notification on the same socket
    #[tokio::test]
    async fn ws_jsonrpc_endpoint_handles_requests_and_notifications_on_same_socket() {
        let config = GlobalConfig::default();
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();

        let ws_url = format!("ws://{}/jsonrpc", addrs.rpc);
        let (mut ws_stream, _) = connect_async(&ws_url)
            .await
            .expect("same-socket JSON-RPC WS connect failed");

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.addUri",
            "params": [["https://example.com/unified-ws.bin"]],
        });

        ws_stream
            .send(Message::Text(request.to_string()))
            .await
            .expect("send addUri over WS");

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut saw_response = false;
        let mut saw_notification = false;

        while tokio::time::Instant::now() < deadline && !(saw_response && saw_notification) {
            let maybe_msg =
                tokio::time::timeout(std::time::Duration::from_millis(500), ws_stream.next()).await;

            let Some(Ok(msg)) =
                maybe_msg.expect("WS read timed out before receiving response/notification")
            else {
                panic!("WS stream ended before unified RPC/notification contract was satisfied");
            };

            let text = msg.to_text().expect("WS frame must be text");
            let json: serde_json::Value = serde_json::from_str(text).expect("valid JSON-RPC frame");

            if json.get("id").and_then(|id| id.as_i64()) == Some(1) {
                assert!(
                    json.get("result").is_some(),
                    "RPC response must contain result: {json}"
                );
                saw_response = true;
                continue;
            }

            if json.get("method").and_then(|m| m.as_str()) == Some("aria2.onDownloadStart") {
                let params = json["params"]
                    .as_array()
                    .expect("notification params array");
                assert_eq!(
                    params.len(),
                    1,
                    "notification params must contain one gid object"
                );
                assert!(
                    params[0].get("gid").and_then(|gid| gid.as_str()).is_some(),
                    "notification must include gid: {json}"
                );
                saw_notification = true;
            }
        }

        assert!(
            saw_response,
            "same-socket WS JSON-RPC never returned addUri response"
        );
        assert!(
            saw_notification,
            "same-socket WS JSON-RPC never delivered aria2.onDownloadStart notification"
        );

        cancel.cancel();
    }

    /// When RPC secret auth is enabled, merely upgrading to WebSocket must not
    /// grant notification access. A client should only receive unsolicited
    /// aria2 notifications after authenticating through the JSON-RPC protocol.
    #[tokio::test]
    async fn unauthenticated_ws_socket_does_not_receive_notifications_when_rpc_secret_is_enabled() {
        let config = GlobalConfig {
            rpc_secret: Some("topsecret".into()),
            ..GlobalConfig::default()
        };
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();

        let ws_url = format!("ws://{}/jsonrpc", addrs.rpc);
        let (mut ws_stream, _) = connect_async(&ws_url).await.expect("WS connect failed");

        // Trigger an event from another producer without authenticating this socket.
        let spec = raria_core::engine::AddUriSpec {
            uris: vec!["https://example.com/secret.bin".into()],
            filename: Some("secret.bin".into()),
            dir: std::path::PathBuf::from("/tmp"),
            connections: 1,
        };
        let _handle = engine.add_uri(&spec).unwrap();

        let notification =
            tokio::time::timeout(std::time::Duration::from_millis(500), ws_stream.next()).await;

        assert!(
            notification.is_err(),
            "unauthenticated same-socket WS client should not receive notifications when rpc_secret is enabled"
        );

        cancel.cancel();
    }

    /// After authenticating on the WebSocket JSON-RPC channel, the same socket
    /// should be eligible for unsolicited aria2 notifications.
    #[tokio::test]
    async fn authenticated_ws_socket_receives_notifications_when_rpc_secret_is_enabled() {
        let config = GlobalConfig {
            rpc_secret: Some("topsecret".into()),
            ..GlobalConfig::default()
        };
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();

        let ws_url = format!("ws://{}/jsonrpc", addrs.rpc);
        let (mut ws_stream, _) = connect_async(&ws_url).await.expect("WS connect failed");

        let auth_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "aria2.getVersion",
            "params": ["token:topsecret"],
        });
        ws_stream
            .send(Message::Text(auth_request.to_string()))
            .await
            .expect("send auth request over WS");

        let response = tokio::time::timeout(std::time::Duration::from_secs(1), ws_stream.next())
            .await
            .expect("timed out waiting for WS auth response")
            .expect("stream ended before auth response")
            .expect("WS auth response frame error");
        let response_json: serde_json::Value =
            serde_json::from_str(response.to_text().unwrap()).unwrap();
        assert_eq!(response_json["id"], 7);
        assert!(
            response_json.get("result").is_some(),
            "expected successful auth response"
        );

        let spec = raria_core::engine::AddUriSpec {
            uris: vec!["https://example.com/authenticated.bin".into()],
            filename: Some("authenticated.bin".into()),
            dir: std::path::PathBuf::from("/tmp"),
            connections: 1,
        };
        let _handle = engine.add_uri(&spec).unwrap();

        let notification =
            tokio::time::timeout(std::time::Duration::from_secs(2), ws_stream.next())
                .await
                .expect("timed out waiting for authenticated WS notification")
                .expect("stream ended before notification")
                .expect("WS notification frame error");

        let json: serde_json::Value =
            serde_json::from_str(notification.to_text().unwrap()).unwrap();
        assert_eq!(json["method"], "aria2.onDownloadStart");

        cancel.cancel();
    }

    #[tokio::test]
    async fn ws_upgrade_with_origin_is_rejected_by_default() {
        let config = GlobalConfig::default();
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine, &rpc_config, cancel.clone())
            .await
            .unwrap();

        let mut request = format!("ws://{}/jsonrpc", addrs.rpc)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Origin", "https://ui.example".parse().unwrap());

        let result = connect_async(request).await;
        assert!(
            result.is_err(),
            "browser-style WS upgrade with Origin should be rejected by default"
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn ws_upgrade_with_origin_is_allowed_when_rpc_allow_origin_all_is_enabled() {
        let config = GlobalConfig {
            rpc_allow_origin_all: true,
            ..GlobalConfig::default()
        };
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine, &rpc_config, cancel.clone())
            .await
            .unwrap();

        let mut request = format!("ws://{}/jsonrpc", addrs.rpc)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Origin", "https://ui.example".parse().unwrap());

        let result = connect_async(request).await;
        assert!(
            result.is_ok(),
            "browser-style WS upgrade should succeed when rpc_allow_origin_all is enabled: {result:?}"
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn authenticated_ws_with_origin_receives_notifications_when_origin_and_secret_policies_allow_it()
     {
        let config = GlobalConfig {
            rpc_secret: Some("topsecret".into()),
            rpc_allow_origin_all: true,
            ..GlobalConfig::default()
        };
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();

        let mut request = format!("ws://{}/jsonrpc", addrs.rpc)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Origin", "https://ui.example".parse().unwrap());

        let (mut ws_stream, _) = connect_async(request)
            .await
            .expect("WS connect with allowed Origin failed");

        let auth_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "aria2.getVersion",
            "params": ["token:topsecret"],
        });
        ws_stream
            .send(Message::Text(auth_request.to_string()))
            .await
            .expect("send auth request over WS");

        let response = tokio::time::timeout(std::time::Duration::from_secs(1), ws_stream.next())
            .await
            .expect("timed out waiting for WS auth response")
            .expect("stream ended before auth response")
            .expect("WS auth response frame error");
        let response_json: serde_json::Value =
            serde_json::from_str(response.to_text().unwrap()).unwrap();
        assert_eq!(response_json["id"], 8);
        assert!(response_json.get("result").is_some());

        let spec = raria_core::engine::AddUriSpec {
            uris: vec!["https://example.com/origin-secret.bin".into()],
            filename: Some("origin-secret.bin".into()),
            dir: std::path::PathBuf::from("/tmp"),
            connections: 1,
        };
        let _handle = engine.add_uri(&spec).unwrap();

        let notification =
            tokio::time::timeout(std::time::Duration::from_secs(2), ws_stream.next())
                .await
                .expect("timed out waiting for authenticated WS notification")
                .expect("stream ended before notification")
                .expect("WS notification frame error");

        let json: serde_json::Value =
            serde_json::from_str(notification.to_text().unwrap()).unwrap();
        assert_eq!(json["method"], "aria2.onDownloadStart");

        cancel.cancel();
    }
}
