use anyhow::Result;
use raria_core::progress::DownloadEvent;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;

#[derive(Clone, Debug, Default)]
pub(crate) struct HookConfig {
    pub on_download_start: Option<PathBuf>,
    pub on_download_complete: Option<PathBuf>,
    pub on_download_error: Option<PathBuf>,
}

pub(crate) fn spawn_hook_runner(
    engine: Arc<raria_core::engine::Engine>,
    hooks: HookConfig,
    shutdown: tokio_util::sync::CancellationToken,
) {
    if hooks.on_download_start.is_none()
        && hooks.on_download_complete.is_none()
        && hooks.on_download_error.is_none()
    {
        return;
    }

    let mut rx = engine.event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                received = rx.recv() => {
                    let Ok(event) = received else {
                        continue;
                    };
                    if let Err(error) = handle_event(&engine, &hooks, event).await {
                        warn!(error = %error, "hook execution failed");
                    }
                }
            }
        }
    });
}

async fn handle_event(
    engine: &raria_core::engine::Engine,
    hooks: &HookConfig,
    event: DownloadEvent,
) -> Result<()> {
    match event {
        DownloadEvent::Started { gid } => {
            if let Some(ref script) = hooks.on_download_start {
                run_hook(engine, script, gid).await?;
            }
        }
        DownloadEvent::Complete { gid } => {
            if let Some(ref script) = hooks.on_download_complete {
                run_hook(engine, script, gid).await?;
            }
        }
        DownloadEvent::Error { gid, .. } => {
            if let Some(ref script) = hooks.on_download_error {
                run_hook(engine, script, gid).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn run_hook(
    engine: &raria_core::engine::Engine,
    script: &std::path::Path,
    gid: raria_core::job::Gid,
) -> Result<()> {
    let job = engine
        .registry
        .get(gid)
        .ok_or_else(|| anyhow::anyhow!("job {gid} not found for hook"))?;

    let num_files = job
        .bt_files
        .as_ref()
        .map(|files| files.len())
        .unwrap_or(1)
        .to_string();
    let file_path = job.out_path.to_string_lossy().into_owned();

    tokio::process::Command::new(script)
        .arg(format!("{gid}"))
        .arg(num_files)
        .arg(file_path)
        .spawn()?;
    Ok(())
}

