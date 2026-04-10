// raria-rpc: HTTP + WebSocket server.
//
// Starts an HTTP + WebSocket RPC surface for aria2-compatible JSON-RPC.
//
// Product contract:
// - HTTP JSON-RPC requests are accepted on `/` and `/jsonrpc`
// - WebSocket JSON-RPC requests are accepted on `/` and `/jsonrpc`
// - aria2-style notifications are pushed over the same WebSocket connection
//   used for JSON-RPC requests
//
// jsonrpsee remains the request dispatcher via `RpcModule::raw_json_request`,
// while the transport contract is owned explicitly here so we can provide
// aria2-compatible same-socket notifications.

use crate::methods::{Aria2RpcServer, RpcHandler};
use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use futures::{SinkExt, StreamExt};
use raria_core::engine::Engine;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tower_http::cors::{Any, CorsLayer};

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
    ///
    /// Kept for compatibility with existing callers. Notifications now share
    /// the same underlying listener as `rpc`; clients should connect to `/`
    /// or `/jsonrpc` on this address.
    pub ws_notify: SocketAddr,
}

#[derive(Clone)]
struct RpcAppState {
    module: Arc<jsonrpsee::RpcModule<()>>,
    notify_tx: tokio::sync::broadcast::Sender<String>,
    max_request_size: usize,
    transport_policy: RpcTransportPolicy,
}

#[derive(Clone, Debug)]
struct RpcTransportPolicy {
    rpc_secret: Option<String>,
    rpc_allow_origin_all: bool,
}

impl RpcTransportPolicy {
    fn new(rpc_secret: Option<String>, rpc_allow_origin_all: bool) -> Self {
        Self {
            rpc_secret,
            rpc_allow_origin_all,
        }
    }

    fn allows_ws_upgrade(&self, headers: &HeaderMap) -> bool {
        !headers.contains_key(axum::http::header::ORIGIN) || self.rpc_allow_origin_all
    }

    fn initial_ws_authenticated(&self) -> bool {
        self.rpc_secret.is_none()
    }

    fn observe_ws_request(&self, request_body: &str, already_authenticated: bool) -> bool {
        already_authenticated
            || request_contains_valid_token(request_body, self.rpc_secret.as_deref())
    }

    fn token_valid(&self, token_value: Option<&str>) -> bool {
        let Some(secret) = self.rpc_secret.as_deref() else {
            return true;
        };
        token_value
            .and_then(|token| token.strip_prefix("token:"))
            .map(|token| token == secret)
            .unwrap_or(false)
    }

    fn first_param_token_valid(&self, params: &[serde_json::Value]) -> bool {
        self.token_valid(params.first().and_then(|value| value.as_str()))
    }
}

/// Start the JSON-RPC server and run until the cancel token is triggered.
///
/// Returns the RPC and WS notification addresses.
///
/// Also spawns a WebSocket event push task that broadcasts aria2-compatible
/// notifications (`aria2.onDownloadStart`, `aria2.onDownloadComplete`, etc.)
/// to all connected WebSocket clients on the same listener.
pub async fn start_rpc_server(
    engine: Arc<Engine>,
    config: &RpcServerConfig,
    cancel: CancellationToken,
) -> Result<RpcServerAddrs> {
    let rpc_secret = engine.config.rpc_secret.clone();

    let handler = RpcHandler::new(Arc::clone(&engine));
    let mut module = handler.into_rpc();

    // Register system.multicall — required for AriaNg compatibility.
    register_system_methods(&mut module)?;

    // If rpc_secret is set, wrap the module with token validation.
    // Convert to RpcModule<()> for uniform handling.
    let module = if let Some(ref secret) = rpc_secret {
        let untyped = module.remove_context();
        wrap_module_with_auth(untyped, secret)?
    } else {
        module.remove_context()
    };
    let module = Arc::new(module);

    // Create the notification broadcast channel.
    let (notify_tx, _) = tokio::sync::broadcast::channel::<String>(256);
    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .context("failed to bind RPC server")?;
    let addr = listener
        .local_addr()
        .context("failed to get local RPC address")?;
    info!(%addr, "RPC server listening");

    let app_state = RpcAppState {
        module: Arc::clone(&module),
        notify_tx: notify_tx.clone(),
        max_request_size: 10 * 1024 * 1024,
        transport_policy: RpcTransportPolicy::new(
            rpc_secret,
            engine.config.rpc_allow_origin_all,
        ),
    };
    let mut app = Router::new()
        .route("/", post(handle_http_rpc).get(handle_ws_rpc))
        .route("/jsonrpc", post(handle_http_rpc).get(handle_ws_rpc))
        .with_state(app_state);
    if engine.config.rpc_allow_origin_all {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers(Any);
        app = app.layer(cors);
    }

    // Spawn WebSocket event push loop.
    let ws_cancel = cancel.clone();
    let ws_engine = Arc::clone(&engine);
    tokio::spawn(async move {
        ws_event_push_loop(ws_engine, ws_cancel, notify_tx).await;
    });

    let server_cancel = cancel.clone();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                server_cancel.cancelled().await;
                info!("stopping RPC server");
            })
            .await
            .expect("RPC server task failed");
    });

    Ok(RpcServerAddrs {
        rpc: addr,
        ws_notify: addr,
    })
}

async fn handle_http_rpc(
    State(state): State<RpcAppState>,
    body: Bytes,
) -> Response {
    let Ok(request_body) = std::str::from_utf8(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            "request body must be valid UTF-8",
        )
            .into_response();
    };

    match dispatch_rpc_request(&state, request_body).await {
        Ok(response) => {
            let mut response = response.into_response();
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            response
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            internal_dispatch_error_frame(&error),
        )
            .into_response(),
    }
}

async fn handle_ws_rpc(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<RpcAppState>,
) -> Response {
    if !state.transport_policy.allows_ws_upgrade(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| handle_ws_rpc_client(socket, state))
}

async fn dispatch_rpc_request(state: &RpcAppState, request_body: &str) -> Result<String> {
    let (response, _rx) = state
        .module
        .raw_json_request(request_body, state.max_request_size)
        .await
        .map_err(|e| anyhow::anyhow!("raw JSON-RPC dispatch failed: {e}"))?;
    Ok(response)
}

fn internal_dispatch_error_frame(error: &anyhow::Error) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": -32603,
            "message": format!("internal dispatch error: {error}"),
        },
        "id": serde_json::Value::Null,
    })
    .to_string()
}

async fn handle_ws_rpc_client(socket: WebSocket, state: RpcAppState) {
    let (mut write, mut read) = socket.split();
    let mut notify_rx = state.notify_tx.subscribe();
    let mut authenticated = state.transport_policy.initial_ws_authenticated();

    loop {
        tokio::select! {
            incoming = read.next() => {
                let Some(incoming) = incoming else {
                    break;
                };

                match incoming {
                    Ok(WsMessage::Text(text)) => {
                        authenticated = state
                            .transport_policy
                            .observe_ws_request(text.as_ref(), authenticated);

                        match dispatch_rpc_request(&state, text.as_ref()).await {
                            Ok(response) => {
                                if !response.is_empty() &&
                                   write.send(WsMessage::Text(response)).await.is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                let error_frame = internal_dispatch_error_frame(&error);

                                if write.send(WsMessage::Text(error_frame)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Ok(WsMessage::Ping(payload)) => {
                        if write.send(WsMessage::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(WsMessage::Close(_)) => break,
                    Ok(WsMessage::Binary(_)) | Ok(WsMessage::Pong(_)) => {}
                    Err(error) => {
                        tracing::debug!(error = %error, "WS RPC client stream error");
                        break;
                    }
                }
            }
            notification = notify_rx.recv() => {
                match notification {
                    Ok(text) => {
                        if !authenticated {
                            continue;
                        }
                        if write.send(WsMessage::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(n, "WS RPC client lagged behind notifications");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn request_contains_valid_token(request_body: &str, secret: Option<&str>) -> bool {
    let Some(secret) = secret else {
        return true;
    };

    let Ok(json) = serde_json::from_str::<serde_json::Value>(request_body) else {
        return false;
    };

    request_value_contains_valid_token(&json, secret)
}

fn request_value_contains_valid_token(value: &serde_json::Value, secret: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => map
            .get("params")
            .and_then(|params| params.as_array())
            .and_then(|params| params.first())
            .and_then(|first| first.as_str())
            .and_then(|token| token.strip_prefix("token:"))
            .map(|token| token == secret)
            .unwrap_or(false),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| request_value_contains_valid_token(item, secret)),
        _ => false,
    }
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
    let policy = RpcTransportPolicy::new(Some(secret.to_string()), false);
    let mut outer: jsonrpsee::RpcModule<()> = jsonrpsee::RpcModule::new(());

    let method_names: Vec<String> = inner.method_names().map(|s| s.to_string()).collect();
    info!(count = method_names.len(), "wrapping RPC methods with auth");

    for name in method_names {
        let inner_clone = Arc::clone(&inner);
        let policy = policy.clone();
        let method_name = name.clone();
        // jsonrpsee requires &'static str for method names.
        let static_name: &'static str = Box::leak(name.into_boxed_str());

        outer.register_async_method(static_name, move |params, _ctx, _ext| {
            let inner = Arc::clone(&inner_clone);
            let policy = policy.clone();
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
                if !policy.first_param_token_valid(&param_array) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use raria_core::config::GlobalConfig;

    #[test]
    fn transport_policy_rejects_origin_by_default() {
        let policy = RpcTransportPolicy::new(None, false);
        let mut headers = HeaderMap::new();
        headers.insert(axum::http::header::ORIGIN, HeaderValue::from_static("https://ui.example"));
        assert!(!policy.allows_ws_upgrade(&headers));
    }

    #[test]
    fn transport_policy_allows_origin_when_opted_in() {
        let policy = RpcTransportPolicy::new(None, true);
        let mut headers = HeaderMap::new();
        headers.insert(axum::http::header::ORIGIN, HeaderValue::from_static("https://ui.example"));
        assert!(policy.allows_ws_upgrade(&headers));
    }

    #[test]
    fn transport_policy_marks_ws_authenticated_after_valid_token_request() {
        let policy = RpcTransportPolicy::new(Some("secret".into()), false);
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "aria2.getVersion",
            "params": ["token:secret"],
        }).to_string();

        assert!(!policy.initial_ws_authenticated());
        assert!(policy.observe_ws_request(&request, false));
    }

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
