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

    /// system.listMethods should return a list of all registered RPC methods.
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

        let methods = json["result"].as_array().unwrap();
        assert!(!methods.is_empty());

        // Must include key aria2 methods
        let method_names: Vec<&str> = methods.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(
            method_names.contains(&"aria2.addUri"),
            "missing aria2.addUri"
        );
        assert!(
            method_names.contains(&"aria2.tellStatus"),
            "missing aria2.tellStatus"
        );
        assert!(
            method_names.contains(&"aria2.getVersion"),
            "missing aria2.getVersion"
        );
        assert!(
            method_names.contains(&"system.multicall"),
            "missing system.multicall"
        );
        assert!(
            method_names.contains(&"system.listMethods"),
            "missing system.listMethods"
        );

        cancel.cancel();
    }

    /// system.listNotifications should return aria2 notification method names.
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

        let notifications = json["result"].as_array().unwrap();
        let names: Vec<&str> = notifications.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"aria2.onDownloadStart"));
        assert!(names.contains(&"aria2.onDownloadComplete"));
        assert!(names.contains(&"aria2.onDownloadError"));
        assert!(names.contains(&"aria2.onSourceFailed"));

        cancel.cancel();
    }
}
