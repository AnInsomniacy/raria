use anyhow::Result;
use raria_core::native::{NativeEvent, NativeEventType, TaskId};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;

#[derive(Clone, Debug, Default)]
pub(crate) struct HookConfig {
    pub on_task_start: Option<PathBuf>,
    pub on_task_complete: Option<PathBuf>,
    pub on_task_fail: Option<PathBuf>,
}

pub(crate) fn spawn_hook_runner(
    engine: Arc<raria_core::engine::Engine>,
    hooks: HookConfig,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let on_task_start = hooks.on_task_start.clone();
    for task in engine.registry.by_status(raria_core::job::Status::Active) {
        if let Some(ref script) = on_task_start {
            let engine_ref = Arc::clone(&engine);
            let script = script.clone();
            let task_id = task.task_id.clone();
            tokio::spawn(async move {
                if let Err(error) = run_hook(engine_ref.as_ref(), &script, &task_id).await {
                    warn!(error = %error, "hook execution failed");
                }
            });
        }
    }

    if hooks.on_task_start.is_none()
        && hooks.on_task_complete.is_none()
        && hooks.on_task_fail.is_none()
    {
        return;
    }

    let mut rx = engine.native_event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                received = rx.recv() => {
                    let Ok(event) = received else {
                        continue;
                    };
                    if let Err(error) = handle_native_event(&engine, &hooks, event).await {
                        warn!(error = %error, "hook execution failed");
                    }
                }
            }
        }
    });
}

async fn handle_native_event(
    engine: &raria_core::engine::Engine,
    hooks: &HookConfig,
    event: NativeEvent,
) -> Result<()> {
    let Some(task_id) = event.task_id else {
        return Ok(());
    };
    match event.event_type {
        NativeEventType::TaskStarted => {
            if let Some(ref script) = hooks.on_task_start {
                run_hook(engine, script, &task_id).await?;
            }
        }
        NativeEventType::TaskCompleted => {
            if let Some(ref script) = hooks.on_task_complete {
                run_hook(engine, script, &task_id).await?;
            }
        }
        NativeEventType::TaskFailed => {
            if let Some(ref script) = hooks.on_task_fail {
                run_hook(engine, script, &task_id).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn run_hook(
    engine: &raria_core::engine::Engine,
    script: &std::path::Path,
    task_id: &TaskId,
) -> Result<()> {
    let job = engine
        .registry
        .get_by_task_id(task_id)
        .ok_or_else(|| anyhow::anyhow!("task {} not found for hook", task_id.as_str()))?;

    let num_files = job
        .bt_files
        .as_ref()
        .map(|files| files.len())
        .unwrap_or(1)
        .to_string();
    let file_path = job.out_path.to_string_lossy().into_owned();

    tokio::process::Command::new(script)
        .arg(job.task_id.as_str())
        .arg(num_files)
        .arg(file_path)
        .spawn()?;
    Ok(())
}
