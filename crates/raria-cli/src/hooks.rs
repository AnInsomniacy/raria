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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookTaskContext {
    pub task_id: TaskId,
    pub file_count: usize,
    pub output_path: PathBuf,
}

impl HookTaskContext {
    fn from_native_summary(summary: raria_core::native::NativeTaskSummary) -> Self {
        let file_count = summary.files.len().max(1);
        Self {
            task_id: summary.task_id,
            file_count,
            output_path: summary.output_path,
        }
    }
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
    let context = hook_task_context(engine, task_id)?;

    tokio::process::Command::new(script)
        .arg(context.task_id.as_str())
        .arg(context.file_count.to_string())
        .arg(context.output_path)
        .spawn()?;
    Ok(())
}

pub(crate) fn hook_task_context(
    engine: &raria_core::engine::Engine,
    task_id: &TaskId,
) -> Result<HookTaskContext> {
    engine
        .native_task_summary(task_id)
        .map(HookTaskContext::from_native_summary)
        .map_err(|_| anyhow::anyhow!("task {} not found for hook", task_id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::hook_task_context;
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_core::job::{BtFile, Job};
    use raria_core::native::TaskId;
    use std::path::PathBuf;

    #[test]
    fn hook_context_uses_native_task_projection() {
        let engine = Engine::new(GlobalConfig::default());
        let task_id = TaskId::new();
        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into()],
            PathBuf::from("/downloads/torrent"),
        );
        job.task_id = task_id.clone();
        job.bt_files = Some(vec![
            BtFile {
                index: 0,
                path: PathBuf::from("one.bin"),
                length: 16,
                completed_length: 0,
                selected: true,
            },
            BtFile {
                index: 1,
                path: PathBuf::from("two.bin"),
                length: 32,
                completed_length: 0,
                selected: true,
            },
        ]);
        engine.submit_job(job, None).expect("insert task");

        let context = hook_task_context(&engine, &task_id).expect("hook context");

        assert_eq!(context.task_id, task_id);
        assert_eq!(context.file_count, 2);
        assert_eq!(context.output_path, PathBuf::from("/downloads/torrent"));
    }

    #[test]
    fn hook_context_reports_missing_native_task() {
        let engine = Engine::new(GlobalConfig::default());
        let task_id = TaskId::new();

        let err = hook_task_context(&engine, &task_id).expect_err("missing task must fail");

        assert!(err.to_string().contains(task_id.as_str()));
    }
}
