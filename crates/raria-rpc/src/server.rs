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

    let handler = RpcHandler::new(engine);
    let handle = server.start(handler.into_rpc());

    // Spawn a task that stops the server when cancel is triggered.
    tokio::spawn(async move {
        cancel.cancelled().await;
        info!("stopping RPC server");
        handle.stop().unwrap();
    });

    Ok(addr)
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
}
