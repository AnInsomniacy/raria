use tokio::sync::oneshot;
use tracing::debug;

use crate::{Error, Result};

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

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .try_init();
}
