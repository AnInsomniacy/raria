use std::{net::SocketAddr, time::Duration};

use axum::Router;
use tokio::{net::TcpListener, sync::oneshot};
use tracing::debug;

use crate::{DownloadEngine, Error, RariaConfig, Result, SharedRpcEngine};

pub struct RariaRuntime {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl RariaRuntime {
    pub async fn start() -> Result<Self> {
        init_tracing();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = shutdown_rx.await;
            debug!("runtime stopped");
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            task,
        })
    }

    pub async fn shutdown(mut self) -> Result<()> {
        let tx = self.shutdown_tx.take().ok_or(Error::RuntimeStopped)?;
        let _ = tx.send(());
        let _ = self.task.await;
        Ok(())
    }
}

pub struct RpcServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<()>>,
    worker_task: tokio::task::JoinHandle<Result<()>>,
}

impl RpcServer {
    pub async fn start(
        config: RariaConfig,
        engine: SharedRpcEngine,
        app: Router,
    ) -> Result<(Self, SocketAddr)> {
        init_tracing();
        let addr = if config.rpc_listen_all {
            SocketAddr::from(([0, 0, 0, 0], config.rpc_listen_port))
        } else {
            SocketAddr::from(([127, 0, 0, 1], config.rpc_listen_port))
        };
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let local_addr = listener
            .local_addr()
            .map_err(|error| Error::Download(error.to_string()))?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|error| Error::Download(error.to_string()))
        });
        let worker_config = config.clone();
        let worker_task = tokio::spawn(async move {
            let download_engine = DownloadEngine::new(worker_config);
            loop {
                {
                    let mut rpc = engine.lock().await;
                    download_engine.run_once(&mut rpc).await?;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        Ok((
            Self {
                shutdown_tx: Some(shutdown_tx),
                task,
                worker_task,
            },
            local_addr,
        ))
    }

    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
        self.worker_task.abort();
        Ok(())
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .try_init();
}
