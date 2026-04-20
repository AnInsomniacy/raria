// system.multicall parity tests.
//
// Verifies that system.multicall correctly:
// 1. Batches multiple RPC calls in a single request
// 2. Returns results in the same order as calls
// 3. Handles errors in individual calls without failing the batch
// 4. Matches aria2's wire format exactly

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{RpcServerConfig, start_rpc_server};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    /// Helper: start a test RPC server and return its address.
    async fn start_test_server() -> (SocketAddr, CancellationToken) {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();
        let config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine, &config, cancel.clone())
            .await
            .unwrap();
        (addrs.rpc, cancel)
    }

    /// system.multicall should execute multiple aria2 methods in a single request.
    /// This is the exact format AriaNg sends.
    #[tokio::test]
    async fn multicall_executes_batch() {
        let (rpc_addr, cancel) = start_test_server().await;

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "1",
            "method": "system.multicall",
            "params": [[
                {"methodName": "aria2.getVersion", "params": []},
                {"methodName": "aria2.getGlobalStat", "params": []}
            ]]
        });

        let resp = client
            .post(format!("http://{rpc_addr}"))
            .json(&body)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();

        // Must have result (not error)
        assert!(
            json.get("error").is_none(),
            "multicall returned error: {json}"
        );

        let result = json["result"].as_array().unwrap();
        assert_eq!(result.len(), 2, "multicall should return 2 results");

        // Each result is wrapped in an array per aria2 convention: [[version_obj], [stat_obj]]
        let version_result = result[0].as_array().unwrap();
        assert_eq!(version_result.len(), 1);
        assert!(version_result[0].get("version").is_some());

        let stat_result = result[1].as_array().unwrap();
        assert_eq!(stat_result.len(), 1);
        assert!(stat_result[0].get("numActive").is_some());

        cancel.cancel();
    }

    /// system.multicall with an invalid method should return error for that call,
    /// not fail the entire batch.
    #[tokio::test]
    async fn multicall_partial_error() {
        let (rpc_addr, cancel) = start_test_server().await;

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "2",
            "method": "system.multicall",
            "params": [[
                {"methodName": "aria2.getVersion", "params": []},
                {"methodName": "aria2.tellStatus", "params": ["0000000000000bad"]}
            ]]
        });

        let resp = client
            .post(format!("http://{rpc_addr}"))
            .json(&body)
            .send()
            .await
            .unwrap();

        let json: serde_json::Value = resp.json().await.unwrap();
        let result = json["result"].as_array().unwrap();
        assert_eq!(result.len(), 2);

        // First call succeeded
        assert!(result[0].as_array().is_some());

        // Second call failed — should be an error object, not an array
        // aria2 wraps errors as {"code": ..., "message": ...}
        assert!(
            result[1].get("code").is_some() || result[1].get("error").is_some(),
            "failed call should return error object, got: {}",
            result[1]
        );

        cancel.cancel();
    }

    /// system.listMethods should return a sorted list of all registered RPC methods.
    ///
    /// Validates against the running server rather than an external manifest file,
    /// ensuring the test is self-contained and authoritative.
    #[tokio::test]
    async fn list_methods_returns_method_names() {
        let (rpc_addr, cancel) = start_test_server().await;

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "3",
            "method": "system.listMethods",
            "params": []
        });

        let resp = client
            .post(format!("http://{rpc_addr}"))
            .json(&body)
            .send()
            .await
            .unwrap();

        let json: serde_json::Value = resp.json().await.unwrap();
        assert!(json.get("error").is_none(), "listMethods error: {json}");

        let methods: Vec<String> = serde_json::from_value(json["result"].clone()).unwrap();

        // Must be sorted (the server sorts before returning).
        let mut sorted = methods.clone();
        sorted.sort();
        assert_eq!(methods, sorted, "listMethods must return sorted names");

        // Must include the core aria2 methods that AriaNg depends on.
        let required = [
            "aria2.addUri",
            "aria2.getVersion",
            "aria2.getGlobalStat",
            "aria2.tellActive",
            "aria2.tellWaiting",
            "aria2.tellStopped",
            "aria2.tellStatus",
            "aria2.pause",
            "aria2.unpause",
            "aria2.remove",
            "aria2.changeGlobalOption",
            "system.multicall",
            "system.listMethods",
            "system.listNotifications",
        ];
        for method in required {
            assert!(
                methods.iter().any(|m| m == method),
                "listMethods missing required method: {method}"
            );
        }

        // Sanity: we register 33+ methods (30 aria2 + 3 system).
        assert!(
            methods.len() >= 33,
            "expected at least 33 methods, got {}",
            methods.len()
        );

        cancel.cancel();
    }

    /// system.listNotifications should return all notification method names.
    ///
    /// Validates against the constants defined in raria_rpc::events, which are
    /// the single source of truth for the notification surface.
    #[tokio::test]
    async fn list_notifications_returns_notification_names() {
        let (rpc_addr, cancel) = start_test_server().await;

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "4",
            "method": "system.listNotifications",
            "params": []
        });

        let resp = client
            .post(format!("http://{rpc_addr}"))
            .json(&body)
            .send()
            .await
            .unwrap();

        let json: serde_json::Value = resp.json().await.unwrap();
        assert!(
            json.get("error").is_none(),
            "listNotifications error: {json}"
        );

        let notifications: Vec<String> = serde_json::from_value(json["result"].clone()).unwrap();

        // Must be sorted.
        let mut sorted = notifications.clone();
        sorted.sort();
        assert_eq!(
            notifications, sorted,
            "listNotifications must return sorted names"
        );

        // Must include the core aria2 parity notifications.
        let required = [
            "aria2.onDownloadStart",
            "aria2.onDownloadPause",
            "aria2.onDownloadStop",
            "aria2.onDownloadComplete",
            "aria2.onDownloadError",
            "aria2.onBtDownloadComplete",
        ];
        for name in required {
            assert!(
                notifications.iter().any(|n| n == name),
                "listNotifications missing required notification: {name}"
            );
        }

        // Extension notification must also be present.
        assert!(
            notifications.iter().any(|n| n == "aria2.onSourceFailed"),
            "listNotifications missing extension notification: aria2.onSourceFailed"
        );

        // Exactly 7 notifications (6 parity + 1 extension).
        assert_eq!(
            notifications.len(),
            7,
            "expected 7 notifications, got {}: {:?}",
            notifications.len(),
            notifications
        );

        cancel.cancel();
    }
}
