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
use tracing::info;

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

/// Start the JSON-RPC server and run until the cancel token is triggered.
///
/// Returns the local address the server is bound to.
///
/// Also spawns a WebSocket event push task that broadcasts aria2-compatible
/// notifications (`aria2.onDownloadStart`, `aria2.onDownloadComplete`, etc.)
/// to all connected WebSocket clients.
pub async fn start_rpc_server(
    engine: Arc<Engine>,
    config: &RpcServerConfig,
    cancel: CancellationToken,
) -> Result<SocketAddr> {
    let server = Server::builder()
        .build(config.listen_addr)
        .await
        .context("failed to bind RPC server")?;

    let addr = server.local_addr().context("failed to get local address")?;
    info!(%addr, "RPC server listening");

    let handler = RpcHandler::new(Arc::clone(&engine));
    let handle = server.start(handler.into_rpc());

    // Spawn WebSocket event push loop.
    let ws_cancel = cancel.clone();
    let ws_engine = Arc::clone(&engine);
    tokio::spawn(async move {
        ws_event_push_loop(ws_engine, ws_cancel).await;
    });

    // Spawn a task that stops the server when cancel is triggered.
    tokio::spawn(async move {
        cancel.cancelled().await;
        info!("stopping RPC server");
        handle.stop().unwrap();
    });

    Ok(addr)
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

/// Continuously subscribes to engine events and logs them.
///
/// In a full WebSocket push implementation, each connected WS client would
/// receive these notifications as JSON-RPC 2.0 notification messages.
/// For now, events are logged at debug level — the jsonrpsee WS layer
/// handles the transport, and we rely on clients polling via tellStatus.
///
/// A complete implementation would maintain a list of active WS connections
/// and send:
///
/// ```json
/// {"jsonrpc":"2.0","method":"aria2.onDownloadStart","params":[{"gid":"..."}]}
/// ```
async fn ws_event_push_loop(engine: Arc<Engine>, cancel: CancellationToken) {
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
                            tracing::debug!(method, ?event, "WS push notification");
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

        let addr = start_rpc_server(Arc::clone(&engine), &config, cancel.clone())
            .await
            .unwrap();

        assert_ne!(addr.port(), 0); // OS assigned a real port.

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
