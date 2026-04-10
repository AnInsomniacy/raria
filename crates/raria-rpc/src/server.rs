// raria-rpc: HTTP + WebSocket server.
//
// Starts a jsonrpsee server that accepts aria2-compatible JSON-RPC requests
// over HTTP and WebSocket.

use crate::methods::{Aria2RpcServer, RpcHandler};
use anyhow::{Context, Result};
use jsonrpsee::server::Server;
use raria_core::engine::Engine;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Configuration for the RPC server.
#[derive(Debug, Clone)]
pub struct RpcServerConfig {
    /// Address to listen on.
    pub listen_addr: SocketAddr,
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 6800)),
        }
    }
}

/// Addresses returned by `start_rpc_server`.
#[derive(Debug, Clone)]
pub struct RpcServerAddrs {
    /// JSON-RPC HTTP/WS address.
    pub rpc: SocketAddr,
    /// WebSocket notification push address.
    pub ws_notify: SocketAddr,
}

/// Start the JSON-RPC server and run until the cancel token is triggered.
///
/// Returns the RPC and WS notification addresses.
///
/// Also spawns a WebSocket event push task that broadcasts aria2-compatible
/// notifications (`aria2.onDownloadStart`, `aria2.onDownloadComplete`, etc.)
/// to all connected WebSocket clients on the notification endpoint.
pub async fn start_rpc_server(
    engine: Arc<Engine>,
    config: &RpcServerConfig,
    cancel: CancellationToken,
) -> Result<RpcServerAddrs> {
    let rpc_secret = engine.config.rpc_secret.clone();

    let server = Server::builder()
        .build(config.listen_addr)
        .await
        .context("failed to bind RPC server")?;

    let addr = server.local_addr().context("failed to get local address")?;
    info!(%addr, "RPC server listening");

    let handler = RpcHandler::new(Arc::clone(&engine));
    let mut module = handler.into_rpc();

    // Register system.multicall — required for AriaNg compatibility.
    register_system_methods(&mut module)?;

    // If rpc_secret is set, wrap the module with token validation.
    // Convert to RpcModule<()> for uniform handling.
    let module = if let Some(secret) = rpc_secret {
        let untyped = module.remove_context();
        wrap_module_with_auth(untyped, &secret)?
    } else {
        module.remove_context()
    };

    let handle = server.start(module);

    // Create the notification broadcast channel.
    let (notify_tx, _) = tokio::sync::broadcast::channel::<String>(256);

    // Prefer port+1 for parity with aria2-style tooling, but fall back to an
    // OS-assigned port if that address is unavailable.
    let preferred_notify_port = if config.listen_addr.port() == 0 {
        0
    } else {
        config.listen_addr.port() + 1
    };
    let preferred_notify_addr =
        SocketAddr::from((config.listen_addr.ip(), preferred_notify_port));

    let ws_notify_addr = match start_ws_notify_listener(
        preferred_notify_addr,
        notify_tx.clone(),
        cancel.clone(),
    )
    .await
    {
        Ok(addr) => addr,
        Err(error) if preferred_notify_port != 0 => {
            warn!(
                preferred = %preferred_notify_addr,
                error = %error,
                "preferred WS notification port unavailable, falling back to an OS-assigned port"
            );
            start_ws_notify_listener(
                SocketAddr::from((config.listen_addr.ip(), 0)),
                notify_tx.clone(),
                cancel.clone(),
            )
            .await
            .context("failed to start WS notify server on fallback port")?
        }
        Err(error) => {
            return Err(error).context("failed to start WS notify server");
        }
    };
    info!(%ws_notify_addr, "WS notification server ready");

    // Spawn WebSocket event push loop.
    let ws_cancel = cancel.clone();
    let ws_engine = Arc::clone(&engine);
    tokio::spawn(async move {
        ws_event_push_loop(ws_engine, ws_cancel, notify_tx).await;
    });

    // Spawn a task that stops the server when cancel is triggered.
    tokio::spawn(async move {
        cancel.cancelled().await;
        info!("stopping RPC server");
        handle.stop().unwrap();
    });

    Ok(RpcServerAddrs {
        rpc: addr,
        ws_notify: ws_notify_addr,
    })
}

/// Wrap an RpcModule with token-based authentication.
///
/// Creates a proxy module that intercepts every method call, validates
/// the `token:<secret>` first parameter (as per aria2 convention),
/// strips it, and forwards the cleaned request to the original module.
fn wrap_module_with_auth(
    inner: jsonrpsee::RpcModule<()>,
    secret: &str,
) -> Result<jsonrpsee::RpcModule<()>> {
    use jsonrpsee::types::ErrorObjectOwned;

    let inner = Arc::new(inner);
    let secret_str = secret.to_string();
    let mut outer: jsonrpsee::RpcModule<()> = jsonrpsee::RpcModule::new(());

    let method_names: Vec<String> = inner.method_names().map(|s| s.to_string()).collect();
    info!(count = method_names.len(), "wrapping RPC methods with auth");

    for name in method_names {
        let inner_clone = Arc::clone(&inner);
        let secret_clone = secret_str.clone();
        let method_name = name.clone();
        // jsonrpsee requires &'static str for method names.
        let static_name: &'static str = Box::leak(name.into_boxed_str());

        outer.register_async_method(static_name, move |params, _ctx, _ext| {
            let inner = Arc::clone(&inner_clone);
            let secret = secret_clone.clone();
            let method = method_name.clone();

            async move {
                // Parse raw params as an array of JSON values.
                let param_array: Vec<serde_json::Value> = match params.parse() {
                    Ok(arr) => arr,
                    Err(_) => {
                        // Might be empty params — reject since we need a token.
                        return Err(ErrorObjectOwned::owned(
                            -32600_i32,
                            "Unauthorized: missing token parameter".to_string(),
                            None::<()>,
                        ));
                    }
                };

                // First param must be "token:<secret>".
                let token_valid = param_array.first()
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.strip_prefix("token:"))
                    .map(|t| t == secret)
                    .unwrap_or(false);

                if !token_valid {
                    return Err(ErrorObjectOwned::owned(
                        -32600_i32,
                        "Unauthorized: invalid or missing token".to_string(),
                        None::<()>,
                    ));
                }

                // Strip the token and forward to the real handler.
                let stripped_params = &param_array[1..];
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": method,
                    "params": stripped_params,
                });

                let (response_str, _rx) = inner
                    .raw_json_request(&request.to_string(), 10 * 1024 * 1024)
                    .await
                    .map_err(|e| ErrorObjectOwned::owned(
                        -32603_i32,
                        format!("internal dispatch error: {e}"),
                        None::<()>,
                    ))?;

                // Parse response and extract result or error.
                let resp: serde_json::Value = serde_json::from_str(&response_str)
                    .map_err(|e| ErrorObjectOwned::owned(
                        -32603_i32,
                        format!("internal response parse error: {e}"),
                        None::<()>,
                    ))?;

                if let Some(error) = resp.get("error") {
                    let code = error["code"].as_i64().unwrap_or(-32000) as i32;
                    let msg = error["message"].as_str().unwrap_or("unknown error");
                    return Err(ErrorObjectOwned::owned(code, msg.to_string(), None::<()>));
                }

                Ok(resp["result"].clone())
            }
        })?;
    }

    Ok(outer)
}

/// Notification method names that aria2 supports.
const ARIA2_NOTIFICATIONS: &[&str] = &[
    "aria2.onDownloadStart",
    "aria2.onDownloadPause",
    "aria2.onDownloadStop",
    "aria2.onDownloadComplete",
    "aria2.onDownloadError",
    "aria2.onBtDownloadComplete",
];

/// Register system.multicall, system.listMethods, system.listNotifications
/// on the RPC module.
///
/// These are required for AriaNg and Motrix compatibility:
/// - AriaNg sends every poll as a system.multicall batch
/// - system.listMethods is used for capability discovery
fn register_system_methods(
    module: &mut jsonrpsee::RpcModule<RpcHandler>,
) -> Result<()> {
    // Collect method names before registering system.* methods.
    let method_names: Vec<String> = module
        .method_names()
        .map(String::from)
        .collect();

    // system.listMethods — returns all registered method names.
    let names_for_list = method_names.clone();
    module
        .register_method("system.listMethods", move |_params, _ctx, _| {
            let mut all_names = names_for_list.clone();
            all_names.push("system.multicall".into());
            all_names.push("system.listMethods".into());
            all_names.push("system.listNotifications".into());
            all_names.sort();
            serde_json::to_value(&all_names)
                .map_err(|e| jsonrpsee::types::ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
        })
        .context("failed to register system.listMethods")?;

    // system.listNotifications — returns notification method names.
    module
        .register_method("system.listNotifications", move |_params, _ctx, _| {
            serde_json::to_value(ARIA2_NOTIFICATIONS)
                .map_err(|e| jsonrpsee::types::ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
        })
        .context("failed to register system.listNotifications")?;

    // system.multicall — aria2's batch execution method.
    //
    // Approach: capture a clone of the RpcModule and dispatch each sub-call
    // by constructing a proper JSON-RPC batch and using raw_json_request.
    let methods_module = module.clone();
    module
        .register_async_method("system.multicall", move |params, _ctx, _ext| {
            let inner_module = methods_module.clone();
            async move {
                // aria2 wraps calls as: params = [[{methodName, params}, ...]]
                let raw: serde_json::Value = params.parse()
                    .map_err(|e: jsonrpsee::types::ErrorObjectOwned| e)?;

                let calls = raw
                    .as_array()
                    .and_then(|outer| outer.first())
                    .and_then(|inner| inner.as_array())
                    .or_else(|| raw.as_array())
                    .ok_or_else(|| jsonrpsee::types::ErrorObjectOwned::owned(
                        -32602, "multicall params must be [[{methodName, params}, ...]]", None::<()>,
                    ))?
                    .clone();

                let mut results = Vec::with_capacity(calls.len());

                for (i, call) in calls.iter().enumerate() {
                    let method_name = call
                        .get("methodName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    let call_params = call
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::json!([]));

                    // Build a JSON-RPC 2.0 request and dispatch via the module.
                    let request = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": i,
                        "method": method_name,
                        "params": call_params
                    });

                    let request_str = serde_json::to_string(&request).unwrap();
                    match inner_module.raw_json_request(&request_str, 1).await {
                        Ok((resp_str, _)) => {
                            if let Ok(resp_json) = serde_json::from_str::<serde_json::Value>(&resp_str) {
                                if let Some(result) = resp_json.get("result") {
                                    results.push(serde_json::json!([result.clone()]));
                                } else if let Some(error) = resp_json.get("error") {
                                    results.push(error.clone());
                                } else {
                                    results.push(serde_json::json!({"code": -32603, "message": "no result or error"}));
                                }
                            } else {
                                results.push(serde_json::json!({"code": -32603, "message": "response parse error"}));
                            }
                        }
                        Err(_) => {
                            results.push(serde_json::json!({"code": -32601, "message": format!("method not found: {method_name}")}));
                        }
                    }
                }

                Ok::<serde_json::Value, jsonrpsee::types::ErrorObjectOwned>(serde_json::json!(results))
            }
        })
        .context("failed to register system.multicall")?;

    Ok(())
}

/// Maps DownloadEvent variants to aria2-compatible WebSocket notification method names.
fn event_to_aria2_method(event: &raria_core::progress::DownloadEvent) -> Option<&'static str> {
    use raria_core::progress::DownloadEvent;
    match event {
        DownloadEvent::Started { .. } => Some("aria2.onDownloadStart"),
        DownloadEvent::Paused { .. } => Some("aria2.onDownloadPause"),
        DownloadEvent::Stopped { .. } => Some("aria2.onDownloadStop"),
        DownloadEvent::Complete { .. } => Some("aria2.onDownloadComplete"),
        DownloadEvent::Error { .. } => Some("aria2.onDownloadError"),
        // StatusChanged and Progress are internal events, not sent as aria2 notifications.
        _ => None,
    }
}

/// Continuously subscribes to engine events and broadcasts them as
/// JSON-RPC 2.0 notifications to all connected WebSocket clients.
///
/// Format sent:
/// ```json
/// {"jsonrpc":"2.0","method":"aria2.onDownloadStart","params":[{"gid":"0000000000000001"}]}
/// ```
async fn ws_event_push_loop(
    engine: Arc<Engine>,
    cancel: CancellationToken,
    notify_tx: tokio::sync::broadcast::Sender<String>,
) {
    let mut rx = engine.event_bus.subscribe();
    info!("WebSocket event push loop started");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("WebSocket event push loop shutting down");
                break;
            }
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Some(method) = event_to_aria2_method(&event) {
                            let gid_str = format!("{:016x}", event_gid(&event).as_raw());
                            let notification = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": method,
                                "params": [{"gid": gid_str}],
                            });
                            let msg = notification.to_string();
                            tracing::debug!(%method, %gid_str, "broadcasting WS notification");
                            // Ignore send errors (no receivers connected).
                            let _ = notify_tx.send(msg);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(n, "WS event push lagged, dropped events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::debug!("event bus closed");
                        break;
                    }
                }
            }
        }
    }
}

/// Extract the GID from a DownloadEvent.
fn event_gid(event: &raria_core::progress::DownloadEvent) -> raria_core::job::Gid {
    use raria_core::progress::DownloadEvent;
    match event {
        DownloadEvent::Started { gid } => *gid,
        DownloadEvent::Paused { gid } => *gid,
        DownloadEvent::Stopped { gid } => *gid,
        DownloadEvent::Complete { gid } => *gid,
        DownloadEvent::Error { gid, .. } => *gid,
        DownloadEvent::Progress { gid, .. } => *gid,
        DownloadEvent::StatusChanged { gid, .. } => *gid,
    }
}

/// Start the WebSocket notification server.
///
/// Listens on `addr` and forwards all messages from `notify_rx` to
/// connected WebSocket clients. Each client gets its own subscriber.
async fn start_ws_notify_listener(
    addr: SocketAddr,
    notify_tx: tokio::sync::broadcast::Sender<String>,
    cancel: CancellationToken,
) -> Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind WS notify listener")?;
    let bound_addr = listener.local_addr()?;
    info!(%bound_addr, "WS notification server listening");

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, peer)) => {
                            tracing::debug!(%peer, "WS notify client connected");
                            let rx = notify_tx.subscribe();
                            let cancel_clone = cancel.clone();
                            tokio::spawn(handle_ws_notify_client(stream, rx, cancel_clone));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "WS notify accept failed");
                        }
                    }
                }
            }
        }
    });

    Ok(bound_addr)
}

/// Handle a single WS notification client connection.
async fn handle_ws_notify_client(
    stream: tokio::net::TcpStream,
    mut rx: tokio::sync::broadcast::Receiver<String>,
    cancel: CancellationToken,
) {
    use futures::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!(error = %e, "WS handshake failed");
            return;
        }
    };

    let (mut write, _read) = futures::StreamExt::split(ws_stream);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if write.send(Message::Text(text.into())).await.is_err() {
                            tracing::debug!("WS notify client disconnected");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(n, "WS notify client lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raria_core::config::GlobalConfig;

    #[tokio::test]
    async fn server_starts_and_stops() {
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let cancel = CancellationToken::new();

        // Use port 0 to get an OS-assigned port.
        let config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };

        let addrs = start_rpc_server(Arc::clone(&engine), &config, cancel.clone())
            .await
            .unwrap();

        assert_ne!(addrs.rpc.port(), 0); // OS assigned a real port.

        // Stop the server.
        cancel.cancel();
        // Give it a moment to clean up.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    #[test]
    fn event_maps_to_aria2_method() {
        use raria_core::job::Gid;
        use raria_core::progress::DownloadEvent;

        assert_eq!(
            event_to_aria2_method(&DownloadEvent::Started {
                gid: Gid::from_raw(1)
            }),
            Some("aria2.onDownloadStart")
        );
        assert_eq!(
            event_to_aria2_method(&DownloadEvent::Complete {
                gid: Gid::from_raw(1)
            }),
            Some("aria2.onDownloadComplete")
        );
        assert_eq!(
            event_to_aria2_method(&DownloadEvent::Paused {
                gid: Gid::from_raw(1)
            }),
            Some("aria2.onDownloadPause")
        );
        assert_eq!(
            event_to_aria2_method(&DownloadEvent::Stopped {
                gid: Gid::from_raw(1)
            }),
            Some("aria2.onDownloadStop")
        );
        assert_eq!(
            event_to_aria2_method(&DownloadEvent::Error {
                gid: Gid::from_raw(1),
                message: "err".into()
            }),
            Some("aria2.onDownloadError")
        );
    }

    #[test]
    fn internal_events_not_mapped() {
        use raria_core::job::{Gid, Status};
        use raria_core::progress::DownloadEvent;

        assert_eq!(
            event_to_aria2_method(&DownloadEvent::Progress {
                gid: Gid::from_raw(1),
                downloaded: 0,
                total: None,
                speed: 0,
            }),
            None
        );
        assert_eq!(
            event_to_aria2_method(&DownloadEvent::StatusChanged {
                gid: Gid::from_raw(1),
                old_status: Status::Waiting,
                new_status: Status::Active,
            }),
            None
        );
    }
}
