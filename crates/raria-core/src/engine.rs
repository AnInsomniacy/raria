// raria-core: Engine — the download orchestrator.
//
// The Engine is the central coordinator that:
// 1. Receives job submissions (add URI)
// 2. Manages the Scheduler queue and concurrency
// 3. Provides lifecycle methods: activate, pause, unpause, complete, fail, remove
// 4. Emits events via EventBus
// 5. Persists all state changes to Store (B1)
// 6. Returns CancellationTokens from activate_job for executor control (B2)
// 7. Handles graceful shutdown via CancellationToken
//
// The Engine does NOT own the download loop itself — that is driven by
// the caller (CLI or daemon) which calls activatable_jobs() and spawns
// SegmentExecutor tasks.

use crate::cancel::CancelRegistry;
use crate::config::GlobalConfig;
use crate::config::JobOptions;
use crate::job::{Gid, Job, Status};
use crate::limiter::SharedRateLimiter;
use crate::logging::emit_structured_log;
use crate::native::{NativeTaskIndex, NativeTaskRow, NativeTaskSummary, TaskId};
use crate::persist::Store;
use crate::progress::{DownloadEvent, EventBus};
use crate::registry::JobRegistry;
use crate::scheduler::Scheduler;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Specification for adding a new download job.
#[derive(Debug, Clone)]
pub struct AddUriSpec {
    /// URIs to download from (multiple = multi-source).
    pub uris: Vec<String>,
    /// Output directory.
    pub dir: PathBuf,
    /// Output filename (if None, derived from URI).
    pub filename: Option<String>,
    /// Number of connections to use.
    pub connections: u32,
}

/// Handle returned when a job is submitted.
#[derive(Debug, Clone)]
pub struct JobHandle {
    /// GID of the newly created job.
    pub gid: Gid,
}

/// Runtime activation handle for a native task.
#[derive(Debug)]
pub struct NativeActivation {
    /// Public native task id.
    pub task_id: TaskId,
    /// Temporary executor bridge id.
    pub runtime_gid: Gid,
    /// Current backend kind.
    pub kind: crate::job::JobKind,
    /// Cancellation token for this activation.
    pub cancel: CancellationToken,
}

/// The download engine.
pub struct Engine {
    /// Thread-safe job index (read/write job metadata by GID).
    pub registry: Arc<JobRegistry>,
    /// FIFO waiting queue with configurable concurrency.
    pub scheduler: Scheduler,
    /// Per-job cancellation tokens.
    pub cancel_registry: CancelRegistry,
    /// Broadcast bus for progress and status events.
    pub event_bus: EventBus,
    /// Global configuration snapshot taken at engine creation.
    pub config: GlobalConfig,
    /// Workspace-wide download rate limiter.
    pub global_rate_limiter: Arc<SharedRateLimiter>,
    /// Per-job limiter handles layered on top of the global limiter.
    job_rate_limiters: Mutex<HashMap<Gid, Arc<SharedRateLimiter>>>,
    /// Native task id index for the current migration runtime.
    native_task_index: Mutex<NativeTaskIndex>,
    /// Unique session identifier (random hex, persisted for lifetime of process).
    pub session_id: String,
    store: Option<Arc<Store>>,
    shutdown: CancellationToken,
    work_notify: Arc<Notify>,
    /// Monotonic timestamp of engine creation (for uptime tracking).
    started_at: Instant,
}

impl Engine {
    /// Create a new Engine with the given configuration (no persistence).
    pub fn new(config: GlobalConfig) -> Self {
        let max_concurrent = config.max_concurrent_downloads;
        let global_rate_limiter =
            Arc::new(SharedRateLimiter::new(config.max_overall_download_limit));
        Self {
            registry: Arc::new(JobRegistry::new()),
            scheduler: Scheduler::new(max_concurrent),
            cancel_registry: CancelRegistry::new(),
            event_bus: EventBus::new(256),
            config,
            global_rate_limiter,
            job_rate_limiters: Mutex::new(HashMap::new()),
            native_task_index: Mutex::new(NativeTaskIndex::default()),
            session_id: format!("{:016x}", rand::random::<u64>()),
            store: None,
            shutdown: CancellationToken::new(),
            work_notify: Arc::new(Notify::new()),
            started_at: Instant::now(),
        }
    }

    /// Create a new Engine with persistence enabled.
    pub fn with_store(config: GlobalConfig, store: Arc<Store>) -> Self {
        let max_concurrent = config.max_concurrent_downloads;
        let global_rate_limiter =
            Arc::new(SharedRateLimiter::new(config.max_overall_download_limit));
        Self {
            registry: Arc::new(JobRegistry::new()),
            scheduler: Scheduler::new(max_concurrent),
            cancel_registry: CancelRegistry::new(),
            event_bus: EventBus::new(256),
            config,
            global_rate_limiter,
            job_rate_limiters: Mutex::new(HashMap::new()),
            native_task_index: Mutex::new(NativeTaskIndex::default()),
            session_id: format!("{:016x}", rand::random::<u64>()),
            store: Some(store),
            shutdown: CancellationToken::new(),
            work_notify: Arc::new(Notify::new()),
            started_at: Instant::now(),
        }
    }

    /// Get a reference to the persistent store, if configured.
    pub fn store(&self) -> Option<&Arc<Store>> {
        self.store.as_ref()
    }

    /// Returns the number of seconds since this engine was created.
    pub fn uptime_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Restore jobs from the persistent store into the in-memory registry.
    ///
    /// - Waiting and Paused jobs are re-enqueued into the scheduler.
    /// - Active / Seeding jobs are demoted to Waiting (the process crashed mid-download).
    /// - Complete, Error, and Removed jobs are loaded but not enqueued.
    pub fn restore(&self) -> Result<usize> {
        let store = self
            .store
            .as_ref()
            .context("restore called without a store")?;

        let native_rows = store
            .list_native_tasks()
            .context("failed to list native task rows from store")?;
        let jobs_with_task_ids = if native_rows.is_empty() {
            store
                .list_jobs()
                .context("failed to list jobs from store")?
                .into_iter()
                .map(|job| (job, None))
                .collect::<Vec<_>>()
        } else {
            native_rows
                .iter()
                .map(|row| {
                    row.to_job_for_migration()
                        .map(|job| (job, Some(row.task_id.clone())))
                })
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to restore native task rows")?
        };
        let count = jobs_with_task_ids.len();

        for (mut job, task_id) in jobs_with_task_ids {
            let gid = job.gid;
            if let Some(task_id) = task_id {
                job.task_id = task_id.clone();
                self.native_task_index.lock().register(task_id, gid);
            } else {
                self.native_task_index
                    .lock()
                    .register(job.task_id.clone(), gid);
            }
            let task_id_for_queue = job.task_id.clone();
            match job.status {
                Status::Active | Status::Seeding => {
                    // Process crashed while downloading or seeding — demote to Waiting.
                    warn!(%gid, "restoring active-like job as waiting (process crash recovery)");
                    emit_structured_log(
                        "WARN",
                        "raria::engine",
                        "restoring active-like job as waiting",
                        [("gid", gid.to_string())],
                    );
                    job.status = Status::Waiting;
                    self.registry.load_from(vec![job]);
                    self.cancel_registry.register(gid);
                    self.scheduler.enqueue_task(task_id_for_queue);
                }
                Status::Waiting => {
                    self.registry.load_from(vec![job]);
                    self.cancel_registry.register(gid);
                    self.scheduler.enqueue_task(task_id_for_queue);
                }
                Status::Paused => {
                    // Paused jobs stay paused but are available for unpause.
                    self.registry.load_from(vec![job]);
                }
                Status::Complete | Status::Error | Status::Removed => {
                    // Terminal states — load for history but don't enqueue.
                    self.registry.load_from(vec![job]);
                }
            }
        }

        info!(count, "restored jobs from store");
        emit_structured_log(
            "INFO",
            "raria::engine",
            "restored jobs from store",
            [("count", count.to_string())],
        );
        self.work_notify.notify_one();
        Ok(count)
    }

    /// Resolve a native task id to the current runtime job id.
    pub fn gid_for_task_id(&self, task_id: &TaskId) -> Option<Gid> {
        self.registry
            .gid_for_task_id(task_id)
            .or_else(|| self.native_task_index.lock().gid_for_task_id(task_id))
    }

    /// Register an existing runtime job under a native task id during migration.
    pub fn register_native_task_id_for_migration(&self, task_id: TaskId, gid: Gid) -> bool {
        if self.registry.get(gid).is_none() {
            return false;
        }
        self.registry.update(gid, |job| {
            job.task_id = task_id.clone();
        });
        self.native_task_index.lock().register(task_id.clone(), gid);
        true
    }

    /// Resolve a runtime job id to the native task id.
    pub fn task_id_for_gid(&self, gid: Gid) -> Option<TaskId> {
        self.native_task_index.lock().task_id_for_gid(gid)
    }

    /// Return a native task projection by native task id.
    pub fn native_task_summary(&self, task_id: &TaskId) -> Result<NativeTaskSummary> {
        let gid = self
            .registry
            .gid_for_task_id(task_id)
            .or_else(|| self.native_task_index.lock().gid_for_task_id(task_id))
            .context("native task not found")?;
        let job = self
            .registry
            .get_by_task_id(task_id)
            .or_else(|| self.registry.get(gid))
            .context("native task not found")?;
        Ok(self.native_task_summary_from_job(&job))
    }

    /// Return all native task projections.
    pub fn native_task_summaries(&self) -> Vec<NativeTaskSummary> {
        self.registry
            .snapshot()
            .iter()
            .map(|job| self.native_task_summary_from_job(job))
            .collect()
    }

    /// Submit a new task through the native task facade.
    pub fn add_native_task(&self, spec: &AddUriSpec) -> Result<NativeTaskSummary> {
        let task_id = TaskId::new();
        let _handle = self.add_uri_with_task_id(spec, None, Some(task_id.clone()))?;
        self.native_task_summary(&task_id)
    }

    /// Pause a task through the native task facade.
    pub fn pause_native_task(&self, task_id: &TaskId) -> Result<NativeTaskSummary> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.pause(gid)?;
        self.native_task_summary(task_id)
    }

    /// Resume a task through the native task facade.
    pub fn resume_native_task(&self, task_id: &TaskId) -> Result<NativeTaskSummary> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.unpause(gid)?;
        self.native_task_summary(task_id)
    }

    /// Remove a task through the native task facade.
    pub fn remove_native_task(&self, task_id: &TaskId) -> Result<NativeTaskSummary> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.force_remove(gid)?;
        self.native_task_summary(task_id)
    }

    /// Restart a task through the native task facade.
    pub fn restart_native_task(&self, task_id: &TaskId) -> Result<NativeTaskSummary> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.registry
            .update(gid, |job| {
                job.status = Status::Waiting;
                job.error_msg = None;
                job.downloaded = 0;
                job.download_speed = 0;
                job.upload_speed = 0;
            })
            .context("native task not found")?;
        self.cancel_registry.register(gid);
        self.scheduler.enqueue_task(task_id.clone());
        self.persist_job_by_gid(gid);
        self.event_bus.publish(DownloadEvent::Started { gid });
        self.work_notify.notify_one();
        self.native_task_summary(task_id)
    }

    fn native_task_summary_from_job(&self, job: &Job) -> NativeTaskSummary {
        let mut summary = NativeTaskSummary::from_job_for_migration(job);
        summary.task_id = job.task_id.clone();
        summary
    }

    /// Insert a prepared job into the engine and waiting queue.
    pub fn submit_job(&self, job: Job, queue_position: Option<usize>) -> Result<JobHandle> {
        let gid = job.gid;
        let task_id = job.task_id.clone();

        // Persist BEFORE in-memory state so crash-safe.
        self.persist_job(&job);

        self.cancel_registry.register(gid);
        self.native_task_index.lock().register(task_id.clone(), gid);
        self.registry
            .insert(job)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to insert job into registry")?;
        if let Some(position) = queue_position {
            self.scheduler.enqueue_task_at(task_id, position);
        } else {
            self.scheduler.enqueue_task(task_id);
        }

        self.event_bus.publish(DownloadEvent::Started { gid });
        self.work_notify.notify_one();
        info!(%gid, queue_position = ?queue_position, "job added");
        emit_structured_log(
            "INFO",
            "raria::engine",
            "job added",
            [
                ("gid", gid.to_string()),
                (
                    "queue_position",
                    queue_position
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "append".to_string()),
                ),
            ],
        );
        Ok(JobHandle { gid })
    }

    /// Submit a new URI download job. Returns the GID.
    pub fn add_uri(&self, spec: &AddUriSpec) -> Result<JobHandle> {
        self.add_uri_with_position(spec, None)
    }

    /// Submit a new URI download job at a specific waiting-queue position.
    pub fn add_uri_with_position(
        &self,
        spec: &AddUriSpec,
        queue_position: Option<usize>,
    ) -> Result<JobHandle> {
        self.add_uri_with_task_id(spec, queue_position, None)
    }

    fn add_uri_with_task_id(
        &self,
        spec: &AddUriSpec,
        queue_position: Option<usize>,
        task_id: Option<TaskId>,
    ) -> Result<JobHandle> {
        let filename = spec
            .filename
            .clone()
            .or_else(|| {
                spec.uris.first().and_then(|u| {
                    url::Url::parse(u)
                        .ok()
                        .and_then(|url| {
                            url.path_segments()
                                .and_then(|mut segs| segs.next_back().map(|s| s.to_string()))
                        })
                        .filter(|s| !s.is_empty())
                })
            })
            .unwrap_or_else(|| "download".to_string());

        let mut out_path = spec.dir.join(&filename);
        if self.config.auto_file_renaming && !self.config.allow_overwrite && out_path.exists() {
            out_path = crate::rename::auto_rename(&out_path);
        }

        // Detect whether this is a BT job (magnet URI) or a range-based download.
        let is_bt = spec.uris.iter().any(|u| u.starts_with("magnet:"));
        let options = JobOptions {
            out: spec.filename.clone(),
            max_connections: spec.connections.max(1),
            ..JobOptions::default()
        };

        let mut job = if is_bt {
            Job::new_bt_with_options(spec.uris.clone(), out_path, options)
        } else {
            Job::new_range_with_options(spec.uris.clone(), out_path, options)
        };
        job.task_id = task_id.unwrap_or_else(|| TaskId::from_migration_gid(job.gid.as_raw()));
        self.submit_job(job, queue_position)
    }

    /// Pause an active job.
    pub fn pause(&self, gid: Gid) -> Result<()> {
        self.registry
            .update(gid, |job| {
                job.transition(Status::Paused)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .context("job not found")?
            .context("pause failed")?;

        self.cancel_registry.cancel(gid);
        if let Some(task_id) = self.task_id_for_gid(gid) {
            self.scheduler.dequeue_task(&task_id);
        }

        self.persist_job_by_gid(gid);
        self.event_bus.publish(DownloadEvent::Paused { gid });
        info!(%gid, "job paused");
        emit_structured_log(
            "INFO",
            "raria::engine",
            "job paused",
            [("gid", gid.to_string())],
        );
        Ok(())
    }

    /// Unpause (resume) a paused job.
    pub fn unpause(&self, gid: Gid) -> Result<()> {
        self.registry
            .update(gid, |job| {
                job.transition(Status::Waiting)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .context("job not found")?
            .context("unpause failed")?;

        self.cancel_registry.register(gid);
        if let Some(task_id) = self.task_id_for_gid(gid) {
            self.scheduler.enqueue_task(task_id);
        }

        self.persist_job_by_gid(gid);
        self.work_notify.notify_one();
        self.event_bus.publish(DownloadEvent::Started { gid });
        info!(%gid, "job resumed");
        emit_structured_log(
            "INFO",
            "raria::engine",
            "job resumed",
            [("gid", gid.to_string())],
        );
        Ok(())
    }

    /// Remove a job (any state → Removed).
    pub fn remove(&self, gid: Gid) -> Result<()> {
        self.cancel_registry.cancel(gid);
        if let Some(task_id) = self.task_id_for_gid(gid) {
            self.scheduler.dequeue_task(&task_id);
        }

        self.registry
            .update(gid, |job| {
                job.transition(Status::Removed)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .context("job not found")?
            .context("remove failed")?;

        self.persist_job_by_gid(gid);
        self.event_bus.publish(DownloadEvent::Stopped { gid });
        info!(%gid, "job removed");
        emit_structured_log(
            "INFO",
            "raria::engine",
            "job removed",
            [("gid", gid.to_string())],
        );
        self.clear_job_rate_limiter(gid);
        Ok(())
    }

    /// Get the GIDs eligible for activation (based on concurrency limit).
    pub fn activatable_jobs(&self) -> Vec<Gid> {
        self.scheduler.jobs_to_activate(&self.registry)
    }

    /// Get the native task ids eligible for activation.
    pub fn activatable_native_tasks(&self) -> Vec<TaskId> {
        self.scheduler.native_tasks_to_activate(&self.registry)
    }

    /// Transition a native task from queued to running.
    pub fn activate_native_task(&self, task_id: &TaskId) -> Result<NativeActivation> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        let cancel = self.activate_job(gid)?;
        let job = self.registry.get(gid).context("native task not found")?;
        Ok(NativeActivation {
            task_id: job.task_id,
            runtime_gid: gid,
            kind: job.kind,
            cancel,
        })
    }

    /// Transition a job from Waiting → Active.
    ///
    /// Returns the CancellationToken for this job so the caller can pass it
    /// to the SegmentExecutor. Cancelling this token will stop the download.
    pub fn activate_job(&self, gid: Gid) -> Result<CancellationToken> {
        self.registry
            .update(gid, |job| {
                job.transition(Status::Active)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .context("job not found")?
            .context("activation failed")?;

        if let Some(task_id) = self.task_id_for_gid(gid) {
            self.scheduler.dequeue_task(&task_id);
        }
        self.persist_job_by_gid(gid);
        self.event_bus.publish(DownloadEvent::Started { gid });
        debug!(%gid, "job activated");

        // Return the cancel token for this job.
        // If one doesn't exist (shouldn't happen), create one.
        let token = self.cancel_registry.child_token(gid).unwrap_or_else(|| {
            warn!(%gid, "no cancel token found during activation, creating one");
            self.cancel_registry.register(gid)
        });
        Ok(token)
    }

    /// Mark a job as complete (Active → Complete).
    pub fn complete_job(&self, gid: Gid) -> Result<()> {
        self.registry
            .update(gid, |job| {
                job.transition(Status::Complete)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .context("job not found")?
            .context("complete transition failed")?;

        self.cancel_registry.remove(gid);
        self.persist_job_by_gid(gid);
        self.event_bus.publish(DownloadEvent::Complete { gid });
        info!(%gid, "job completed");
        emit_structured_log(
            "INFO",
            "raria::engine",
            "job completed",
            [("gid", gid.to_string())],
        );
        self.clear_job_rate_limiter(gid);
        self.work_notify.notify_one();
        Ok(())
    }

    /// Mark a job as failed (Active → Error).
    pub fn fail_job(&self, gid: Gid, error_msg: &str) -> Result<()> {
        let msg = error_msg.to_string();
        self.registry
            .update(gid, |job| -> anyhow::Result<()> {
                job.transition(Status::Error)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                job.error_msg = Some(msg.clone());
                Ok(())
            })
            .context("job not found")?
            .context("error transition failed")?;

        self.cancel_registry.remove(gid);
        self.persist_job_by_gid(gid);
        self.event_bus.publish(DownloadEvent::Error {
            gid,
            message: error_msg.to_string(),
        });
        error!(%gid, error_msg, "job failed");
        emit_structured_log(
            "ERROR",
            "raria::engine",
            "job failed",
            [("gid", gid.to_string()), ("error", error_msg.to_string())],
        );
        self.clear_job_rate_limiter(gid);
        self.work_notify.notify_one();
        Ok(())
    }

    /// Record that a single source failed while the job may continue with others.
    pub fn source_failed(&self, gid: Gid, uri: &str, error_msg: &str) -> Result<()> {
        self.registry.get(gid).context("job not found")?;
        self.event_bus.publish(DownloadEvent::SourceFailed {
            gid,
            uri: uri.to_string(),
            message: error_msg.to_string(),
        });
        warn!(%gid, uri, error = error_msg, "job source failed");
        emit_structured_log(
            "WARN",
            "raria::engine",
            "job source failed",
            [
                ("gid", gid.to_string()),
                ("uri", uri.to_string()),
                ("error", error_msg.to_string()),
            ],
        );
        Ok(())
    }

    /// Mutate the URI list attached to a single-file download.
    ///
    /// Removal happens before insertion. When `position` is omitted, new URIs
    /// are appended to the remaining list.
    pub fn change_uris(
        &self,
        gid: Gid,
        file_index: usize,
        del_uris: &[String],
        add_uris: &[String],
        position: Option<usize>,
    ) -> Result<(usize, usize)> {
        let outcome = self
            .registry
            .update(gid, |job| -> anyhow::Result<(usize, usize, bool)> {
                anyhow::ensure!(file_index > 0, "fileIndex must be 1-based");
                anyhow::ensure!(
                    job.kind != crate::job::JobKind::Bt,
                    "changeUri is not supported for BitTorrent jobs"
                );

                let file_count = if job.kind == crate::job::JobKind::Bt {
                    job.bt_files.as_ref().map(|files| files.len()).unwrap_or(1)
                } else {
                    1
                };
                anyhow::ensure!(
                    file_index <= file_count,
                    "fileIndex {file_index} is out of range for this download"
                );
                anyhow::ensure!(
                    file_count == 1,
                    "per-file URI mutation is not supported for multi-file downloads"
                );

                let mut deleted = 0usize;
                for uri in del_uris {
                    if let Some(index) = job.uris.iter().position(|candidate| candidate == uri) {
                        job.uris.remove(index);
                        deleted += 1;
                    }
                }

                let insert_at = position.unwrap_or(job.uris.len()).min(job.uris.len());
                let mut added = 0usize;
                for uri in add_uris {
                    if url::Url::parse(uri).is_err() {
                        continue;
                    }
                    job.uris.insert(insert_at + added, uri.clone());
                    added += 1;
                }

                Ok((
                    deleted,
                    added,
                    matches!(job.status, Status::Waiting | Status::Paused),
                ))
            })
            .context("job not found")?
            .map_err(|error| anyhow::anyhow!("changeUri failed: {error}"))?;

        let (deleted, added, should_notify) = outcome;
        self.persist_job_by_gid(gid);
        if should_notify {
            self.work_notify.notify_one();
        }
        debug!(%gid, deleted, added, ?position, "changed job URIs");
        Ok((deleted, added))
    }

    /// Get or create the shared per-job limiter layered on top of the global limiter.
    pub fn job_rate_limiter(&self, gid: Gid, limit_bps: u64) -> Arc<SharedRateLimiter> {
        let mut handles = self.job_rate_limiters.lock();
        Arc::clone(handles.entry(gid).or_insert_with(|| {
            Arc::new(SharedRateLimiter::chained(
                limit_bps,
                Arc::clone(&self.global_rate_limiter),
            ))
        }))
    }

    /// Hot-update the per-job limiter for a running or future download.
    pub fn update_job_rate_limit(&self, gid: Gid, limit_bps: u64) -> Result<()> {
        self.registry.get(gid).context("job not found")?;
        let limiter = self.job_rate_limiter(gid, limit_bps);
        limiter.update_limit(limit_bps);
        Ok(())
    }

    fn clear_job_rate_limiter(&self, gid: Gid) {
        self.job_rate_limiters.lock().remove(&gid);
    }

    /// Update download progress for a job.
    pub fn update_progress(&self, gid: Gid, bytes: u64) {
        let _ = self.registry.update(gid, |job| {
            job.downloaded += bytes;
        });
    }

    /// Get a clone of the shutdown token.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Signal graceful shutdown.
    pub fn shutdown(&self) {
        info!("engine shutting down");
        self.cancel_registry.cancel_all();
        self.shutdown.cancel();
    }

    /// Cancel currently running native tasks while leaving task state persistence to shutdown flow.
    pub fn cancel_active_native_tasks(&self) -> usize {
        let active = self.registry.by_status(Status::Active);
        let seeding = self.registry.by_status(Status::Seeding);
        let mut count = 0;
        for job in active.iter().chain(seeding.iter()) {
            if self.cancel_registry.cancel(job.gid) {
                count += 1;
            }
        }
        count
    }

    /// Update progress through a native task id.
    pub fn update_native_progress(&self, task_id: &TaskId, bytes: u64) -> Result<()> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.update_progress(gid, bytes);
        Ok(())
    }

    /// Set runtime connection count through a native task id.
    pub fn set_native_runtime_connections(&self, task_id: &TaskId, connections: u32) -> Result<()> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.registry
            .update(gid, |job| {
                job.connections = connections;
            })
            .context("native task not found")?;
        Ok(())
    }

    /// Complete a native task after executor success.
    pub fn complete_native_task(&self, task_id: &TaskId, downloaded: u64) -> Result<()> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.registry
            .update(gid, |job| {
                job.downloaded = downloaded;
                job.connections = 0;
            })
            .context("native task not found")?;
        self.complete_job(gid)
    }

    /// Fail a native task after executor failure.
    pub fn fail_native_task(&self, task_id: &TaskId, error_msg: &str) -> Result<()> {
        let gid = self
            .gid_for_task_id(task_id)
            .context("native task not found")?;
        self.registry
            .update(gid, |job| {
                job.connections = 0;
            })
            .context("native task not found")?;
        self.fail_job(gid, error_msg)
    }

    /// Get the work notifier for the run loop.
    pub fn work_notify(&self) -> Arc<Notify> {
        Arc::clone(&self.work_notify)
    }

    // ── Private helpers ─────────────────────────────────────────────────

    /// Persist a job to the store (no-op if store is not configured).
    fn persist_job(&self, job: &Job) {
        if let Some(ref store) = self.store {
            if let Err(e) = store.put_job(job) {
                error!(gid = %job.gid, error = %e, "failed to persist job");
            }
        }
    }

    /// Look up a job by GID and persist it.
    fn persist_job_by_gid(&self, gid: Gid) {
        if let Some(ref store) = self.store {
            if let Some(job) = self.registry.get(gid) {
                if let Err(e) = store.put_job(&job) {
                    error!(%gid, error = %e, "failed to persist job");
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Batch operations (aria2 RPC parity)
// ═══════════════════════════════════════════════════════════════════════

impl Engine {
    /// Pause all active and waiting jobs.
    ///
    /// aria2 equivalent: `aria2.pauseAll`
    /// Returns the number of jobs paused.
    pub fn pause_all(&self) -> usize {
        let active = self.registry.by_status(Status::Active);
        let seeding = self.registry.by_status(Status::Seeding);
        let waiting = self.registry.by_status(Status::Waiting);
        let mut count = 0;

        for job in active.iter().chain(seeding.iter()).chain(waiting.iter()) {
            if self.pause(job.gid).is_ok() {
                count += 1;
            }
        }
        info!(count, "paused all jobs");
        count
    }

    /// Unpause all paused jobs.
    ///
    /// aria2 equivalent: `aria2.unpauseAll`
    /// Returns the number of jobs unpaused.
    pub fn unpause_all(&self) -> usize {
        let paused = self.registry.by_status(Status::Paused);
        let mut count = 0;

        for job in &paused {
            if self.unpause(job.gid).is_ok() {
                count += 1;
            }
        }
        info!(count, "unpaused all jobs");
        count
    }

    /// Force-remove a job. Unlike `remove()`, this also works on Active jobs
    /// that haven't responded to a graceful cancel yet.
    ///
    /// aria2 equivalent: `aria2.forceRemove`
    pub fn force_remove(&self, gid: Gid) -> Result<()> {
        // Cancel first — even if the task is still running.
        self.cancel_registry.cancel(gid);
        if let Some(task_id) = self.task_id_for_gid(gid) {
            self.scheduler.dequeue_task(&task_id);
        }
        self.clear_job_rate_limiter(gid);

        // Force transition to Removed regardless of current state.
        self.registry
            .update(gid, |job| {
                job.status = Status::Removed;
            })
            .context("job not found")?;

        self.persist_job_by_gid(gid);
        self.event_bus.publish(DownloadEvent::Stopped { gid });
        info!(%gid, "job force-removed");
        Ok(())
    }

    /// Remove a single download result (completed/error/removed job).
    ///
    /// aria2 equivalent: `aria2.removeDownloadResult`
    pub fn remove_download_result(&self, gid: Gid) -> Result<()> {
        let job = self.registry.get(gid).context("GID not found")?;
        match job.status {
            Status::Complete | Status::Error | Status::Removed => {
                self.registry.remove(gid);
                self.clear_job_rate_limiter(gid);
                if let Some(ref store) = self.store {
                    if let Err(e) = store.remove_job(gid) {
                        warn!(%gid, error = %e, "failed to delete job from store");
                    }
                    let _ = store.remove_segments(gid);
                }
                debug!(%gid, "download result removed");
                Ok(())
            }
            _ => anyhow::bail!("cannot remove result: job {gid} is {}", job.status_str()),
        }
    }

    /// Purge all completed/error/removed download results.
    ///
    /// aria2 equivalent: `aria2.purgeDownloadResult`
    /// Returns the number of results purged.
    pub fn purge_download_results(&self) -> usize {
        let mut purged = 0;
        let jobs = self.registry.snapshot();
        for job in &jobs {
            match job.status {
                Status::Complete | Status::Error | Status::Removed => {
                    self.registry.remove(job.gid);
                    self.clear_job_rate_limiter(job.gid);
                    if let Some(ref store) = self.store {
                        let _ = store.remove_job(job.gid);
                        let _ = store.remove_segments(job.gid);
                    }
                    purged += 1;
                }
                _ => {}
            }
        }
        info!(purged, "purged download results");
        purged
    }

    /// Change the position of a download in the waiting queue.
    ///
    /// aria2 equivalent: `aria2.changePosition`
    ///
    /// `how` semantics:
    /// - `POS_SET`: Set position to `pos` from the beginning.
    /// - `POS_CUR`: Move relative from current position.
    /// - `POS_END`: Set position to `pos` from the end.
    ///
    /// Returns the new position (0-indexed).
    pub fn change_position(&self, gid: Gid, pos: i32, how: PositionHow) -> Result<usize> {
        let job = self.registry.get(gid).context("GID not found")?;
        if job.status != Status::Waiting {
            anyhow::bail!("changePosition: job {gid} is not waiting");
        }
        let new_pos = self.scheduler.change_position(gid, pos, how)?;
        debug!(%gid, new_pos, "changed position");
        Ok(new_pos)
    }

    /// Save the current session to the store.
    ///
    /// aria2 equivalent: `aria2.saveSession`
    pub fn save_session(&self) -> Result<()> {
        let store = self
            .store
            .as_ref()
            .context("save_session called without a store")?;

        let jobs = self.registry.snapshot();
        for job in &jobs {
            store
                .put_job(job)
                .with_context(|| format!("failed to persist job {}", job.gid))?;
            let mut row = NativeTaskRow::from_job_for_migration(job);
            if let Some(task_id) = self.task_id_for_gid(job.gid) {
                row.task_id = task_id;
            }
            store
                .put_native_task(&row)
                .with_context(|| format!("failed to persist native task row {}", job.gid))?;
        }
        info!(count = jobs.len(), "session saved");
        Ok(())
    }
}

/// Position mode for `change_position`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionHow {
    /// Absolute position from beginning.
    Set,
    /// Relative to current position.
    Cur,
    /// Position from end.
    End,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobKind;

    fn default_config() -> GlobalConfig {
        GlobalConfig {
            max_concurrent_downloads: 5,
            ..Default::default()
        }
    }

    fn default_spec() -> AddUriSpec {
        AddUriSpec {
            uris: vec!["https://example.com/file.zip".into()],
            dir: PathBuf::from("/tmp/downloads"),
            filename: None,
            connections: 16,
        }
    }

    fn engine_with_store() -> (Engine, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let store = Arc::new(Store::open(&db_path).unwrap());
        let engine = Engine::with_store(default_config(), store);
        (engine, dir)
    }

    // ═══════════════════════════════════════════════════════════════════
    // Original engine tests (preserved, adapted for new activate_job sig)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn engine_creates_with_config() {
        let engine = Engine::new(default_config());
        assert_eq!(engine.scheduler.max_concurrent(), 5);
        assert_eq!(engine.registry.len(), 0);
    }

    #[test]
    fn add_uri_creates_waiting_job() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Waiting);
        assert_eq!(job.kind, JobKind::Range);
        assert!(!job.uris.is_empty());
        assert_eq!(engine.scheduler.queue_len(), 1);
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        assert_eq!(engine.gid_for_task_id(&task_id), Some(handle.gid));
    }

    #[test]
    fn register_native_task_id_for_migration_requires_existing_job() {
        let engine = Engine::new(default_config());
        let task_id = TaskId::new();

        assert!(!engine.register_native_task_id_for_migration(task_id.clone(), Gid::from_raw(404)));

        let handle = engine.add_uri(&default_spec()).unwrap();
        assert!(engine.register_native_task_id_for_migration(task_id.clone(), handle.gid));
        assert_eq!(engine.gid_for_task_id(&task_id), Some(handle.gid));
    }

    #[test]
    fn native_task_facade_creates_opaque_task_and_controls_lifecycle() {
        let engine = Engine::new(default_config());

        let created = engine.add_native_task(&default_spec()).unwrap();
        assert!(created.task_id.as_str().starts_with("task_"));
        assert!(!created.task_id.as_str().starts_with("task_migration_"));
        assert_eq!(created.lifecycle, crate::native::TaskLifecycle::Queued);

        let paused = engine.pause_native_task(&created.task_id).unwrap();
        assert_eq!(paused.lifecycle, crate::native::TaskLifecycle::Paused);

        let resumed = engine.resume_native_task(&created.task_id).unwrap();
        assert_eq!(resumed.lifecycle, crate::native::TaskLifecycle::Queued);

        let restarted = engine.restart_native_task(&created.task_id).unwrap();
        assert_eq!(restarted.lifecycle, crate::native::TaskLifecycle::Queued);

        let removed = engine.remove_native_task(&created.task_id).unwrap();
        assert_eq!(removed.lifecycle, crate::native::TaskLifecycle::Removed);
    }

    #[test]
    fn native_activation_uses_task_id_with_runtime_bridge() {
        let engine = Engine::new(default_config());
        let created = engine.add_native_task(&default_spec()).unwrap();

        let activatable = engine.activatable_native_tasks();
        assert_eq!(activatable, vec![created.task_id.clone()]);

        let activation = engine.activate_native_task(&created.task_id).unwrap();
        assert_eq!(activation.task_id, created.task_id);
        assert_eq!(activation.kind, JobKind::Range);
        assert!(!activation.cancel.is_cancelled());

        let job = engine.registry.get(activation.runtime_gid).unwrap();
        assert_eq!(job.status, Status::Active);
    }

    #[test]
    fn cancel_active_native_tasks_cancels_running_tokens_without_public_gid_access() {
        let engine = Engine::new(default_config());
        let created = engine.add_native_task(&default_spec()).unwrap();
        let activation = engine.activate_native_task(&created.task_id).unwrap();

        engine.cancel_active_native_tasks();

        assert!(activation.cancel.is_cancelled());
    }

    #[test]
    fn native_runtime_helpers_update_progress_and_terminal_state() {
        let engine = Engine::new(default_config());
        let created = engine.add_native_task(&default_spec()).unwrap();
        let activation = engine.activate_native_task(&created.task_id).unwrap();

        engine
            .update_native_progress(&created.task_id, 128)
            .unwrap();
        engine
            .set_native_runtime_connections(&created.task_id, 2)
            .unwrap();
        engine.complete_native_task(&created.task_id, 512).unwrap();

        let job = engine.registry.get(activation.runtime_gid).unwrap();
        assert_eq!(job.downloaded, 512);
        assert_eq!(job.connections, 0);
        assert_eq!(job.status, Status::Complete);
    }

    #[test]
    fn add_uri_applies_requested_connection_count_to_job_options() {
        let engine = Engine::new(default_config());
        let handle = engine
            .add_uri(&AddUriSpec {
                connections: 3,
                ..default_spec()
            })
            .unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.options.max_connections, 3);
    }

    #[test]
    fn add_uri_with_position_inserts_into_waiting_queue() {
        let engine = Engine::new(default_config());
        let first = engine.add_uri(&default_spec()).unwrap();
        let second = engine.add_uri(&default_spec()).unwrap();
        let third = engine
            .add_uri_with_position(&default_spec(), Some(1))
            .unwrap();

        assert_eq!(
            engine.scheduler.waiting_queue(),
            vec![first.gid, third.gid, second.gid]
        );
    }

    #[test]
    fn add_uri_extracts_filename() {
        let engine = Engine::new(default_config());
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/path/to/bigfile.tar.gz".into()],
                dir: PathBuf::from("/downloads"),
                filename: None,
                connections: 4,
            })
            .unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.out_path, PathBuf::from("/downloads/bigfile.tar.gz"));
    }

    #[test]
    fn add_uri_uses_explicit_filename() {
        let engine = Engine::new(default_config());
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.com/file.zip".into()],
                dir: PathBuf::from("/output"),
                filename: Some("custom.dat".into()),
                connections: 1,
            })
            .unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.out_path, PathBuf::from("/output/custom.dat"));
    }

    #[test]
    fn activatable_jobs_respects_concurrency() {
        let engine = Engine::new(GlobalConfig {
            max_concurrent_downloads: 2,
            ..Default::default()
        });

        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();
        let _h3 = engine.add_uri(&default_spec()).unwrap();

        let activatable = engine.activatable_jobs();
        assert_eq!(activatable.len(), 2);
        assert_eq!(activatable[0], h1.gid);
        assert_eq!(activatable[1], h2.gid);
    }

    #[test]
    fn activate_job_transitions_status() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        let _token = engine.activate_job(handle.gid).unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Active);
        assert_eq!(engine.scheduler.queue_len(), 0);
    }

    #[test]
    fn pause_unpause_cycle() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        engine.activate_job(handle.gid).unwrap();

        engine.pause(handle.gid).unwrap();
        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Paused);

        engine.unpause(handle.gid).unwrap();
        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Waiting);
        assert_eq!(engine.scheduler.queue_len(), 1);
    }

    #[test]
    fn complete_job_transitions() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();

        engine.complete_job(handle.gid).unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Complete);
    }

    #[test]
    fn fail_job_records_error() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();

        engine.fail_job(handle.gid, "connection timeout").unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Error);
        assert_eq!(job.error_msg.as_deref(), Some("connection timeout"));
    }

    #[tokio::test]
    async fn source_failed_publishes_event_without_changing_job_status() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();
        let mut rx = engine.event_bus.subscribe();

        engine
            .source_failed(
                handle.gid,
                "https://mirror.example/file.zip",
                "permanent error: checksum mismatch",
            )
            .unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for source-failed event")
            .expect("source-failed event should be published");

        match event {
            DownloadEvent::SourceFailed { gid, uri, message } => {
                assert_eq!(gid, handle.gid);
                assert_eq!(uri, "https://mirror.example/file.zip");
                assert_eq!(message, "permanent error: checksum mismatch");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Active);
        assert!(job.error_msg.is_none());
    }

    #[test]
    fn change_uris_removes_then_inserts_at_requested_position() {
        let engine = Engine::new(default_config());
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec![
                    "https://mirror-1.example/file.iso".into(),
                    "https://mirror-2.example/file.iso".into(),
                    "https://mirror-3.example/file.iso".into(),
                ],
                ..default_spec()
            })
            .unwrap();

        let (deleted, added) = engine
            .change_uris(
                handle.gid,
                1,
                &[String::from("https://mirror-2.example/file.iso")],
                &[String::from("https://mirror-new.example/file.iso")],
                Some(0),
            )
            .unwrap();

        assert_eq!((deleted, added), (1, 1));
        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(
            job.uris,
            vec![
                "https://mirror-new.example/file.iso",
                "https://mirror-1.example/file.iso",
                "https://mirror-3.example/file.iso",
            ]
        );
    }

    #[test]
    fn change_uris_skips_invalid_additions_without_failing() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        let (deleted, added) = engine
            .change_uris(
                handle.gid,
                1,
                &[],
                &[
                    String::from("not a uri"),
                    String::from("https://mirror-new.example/file.iso"),
                ],
                None,
            )
            .unwrap();

        assert_eq!((deleted, added), (0, 1));
        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(
            job.uris,
            vec![
                "https://example.com/file.zip",
                "https://mirror-new.example/file.iso",
            ]
        );
    }

    #[test]
    fn change_uris_rejects_bittorrent_jobs() {
        let engine = Engine::new(default_config());
        let job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:abc123".into()],
            PathBuf::from("/tmp/download"),
        );
        let gid = job.gid;
        engine.submit_job(job, None).unwrap();

        let error = engine
            .change_uris(
                gid,
                1,
                &[],
                &[String::from("https://example.com/file")],
                None,
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("changeUri is not supported for BitTorrent jobs")
        );
    }

    #[test]
    fn remove_job_transitions() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();

        engine.remove(handle.gid).unwrap();

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Removed);
    }

    #[test]
    fn update_progress_increments() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        engine.update_progress(handle.gid, 1000);
        engine.update_progress(handle.gid, 2000);

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.downloaded, 3000);
    }

    #[test]
    fn completion_frees_slot() {
        let engine = Engine::new(GlobalConfig {
            max_concurrent_downloads: 1,
            ..Default::default()
        });

        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();

        engine.activate_job(h1.gid).unwrap();
        assert!(engine.activatable_jobs().is_empty());

        engine.complete_job(h1.gid).unwrap();

        let activatable = engine.activatable_jobs();
        assert_eq!(activatable.len(), 1);
        assert_eq!(activatable[0], h2.gid);
    }

    #[test]
    fn shutdown_cancels_token() {
        let engine = Engine::new(default_config());
        let token = engine.shutdown_token();
        assert!(!token.is_cancelled());

        engine.shutdown();
        assert!(token.is_cancelled());
    }

    #[test]
    fn shutdown_cancels_active_job_tokens() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();
        let token = engine.activate_job(handle.gid).unwrap();
        assert!(!token.is_cancelled());

        engine.shutdown();
        assert!(token.is_cancelled());
    }

    // ═══════════════════════════════════════════════════════════════════
    // B1: Engine ↔ Store Persistence tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn engine_persists_job_on_add_uri() {
        let (engine, _dir) = engine_with_store();
        let handle = engine.add_uri(&default_spec()).unwrap();

        // Verify the job was persisted to the store.
        let store = engine.store.as_ref().unwrap();
        let persisted = store
            .get_job(handle.gid)
            .unwrap()
            .expect("job should be in store");
        assert_eq!(persisted.gid, handle.gid);
        assert_eq!(persisted.status, Status::Waiting);
        assert_eq!(persisted.uris, vec!["https://example.com/file.zip"]);
    }

    #[test]
    fn engine_persists_on_activate() {
        let (engine, _dir) = engine_with_store();
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();

        let store = engine.store.as_ref().unwrap();
        let persisted = store.get_job(handle.gid).unwrap().unwrap();
        assert_eq!(persisted.status, Status::Active);
    }

    #[test]
    fn engine_persists_on_complete() {
        let (engine, _dir) = engine_with_store();
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();
        engine.complete_job(handle.gid).unwrap();

        let store = engine.store.as_ref().unwrap();
        let persisted = store.get_job(handle.gid).unwrap().unwrap();
        assert_eq!(persisted.status, Status::Complete);
    }

    #[test]
    fn engine_persists_on_fail() {
        let (engine, _dir) = engine_with_store();
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();
        engine.fail_job(handle.gid, "network error").unwrap();

        let store = engine.store.as_ref().unwrap();
        let persisted = store.get_job(handle.gid).unwrap().unwrap();
        assert_eq!(persisted.status, Status::Error);
        assert_eq!(persisted.error_msg.as_deref(), Some("network error"));
    }

    #[test]
    fn engine_persists_on_pause() {
        let (engine, _dir) = engine_with_store();
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();
        engine.pause(handle.gid).unwrap();

        let store = engine.store.as_ref().unwrap();
        let persisted = store.get_job(handle.gid).unwrap().unwrap();
        assert_eq!(persisted.status, Status::Paused);
    }

    #[test]
    fn engine_persists_on_remove() {
        let (engine, _dir) = engine_with_store();
        let handle = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(handle.gid).unwrap();
        engine.remove(handle.gid).unwrap();

        let store = engine.store.as_ref().unwrap();
        let persisted = store.get_job(handle.gid).unwrap().unwrap();
        assert_eq!(persisted.status, Status::Removed);
    }

    #[test]
    fn engine_restore_loads_waiting_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("restore.redb");
        let store = Arc::new(Store::open(&db_path).unwrap());

        // Phase 1: Add jobs to a first engine instance.
        let engine1 = Engine::with_store(default_config(), Arc::clone(&store));
        let h1 = engine1.add_uri(&default_spec()).unwrap();
        let h2 = engine1.add_uri(&default_spec()).unwrap();
        let gid1 = h1.gid;
        let gid2 = h2.gid;
        drop(engine1);

        // Phase 2: Create a NEW engine and restore.
        let engine2 = Engine::with_store(default_config(), Arc::clone(&store));
        assert_eq!(engine2.registry.len(), 0); // Empty before restore.

        let count = engine2.restore().unwrap();
        assert_eq!(count, 2);
        assert_eq!(engine2.registry.len(), 2);

        // Both should be Waiting and in the scheduler queue.
        let j1 = engine2.registry.get(gid1).unwrap();
        assert_eq!(j1.status, Status::Waiting);
        let j2 = engine2.registry.get(gid2).unwrap();
        assert_eq!(j2.status, Status::Waiting);
        assert_eq!(engine2.scheduler.queue_len(), 2);
    }

    #[test]
    fn engine_restore_prefers_native_task_rows_when_available() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("native-restore.redb");
        let store = Arc::new(Store::open(&db_path).unwrap());
        let engine1 = Engine::with_store(default_config(), Arc::clone(&store));
        let handle = engine1.add_uri(&default_spec()).unwrap();
        let gid = handle.gid;
        let mut native_row =
            NativeTaskRow::from_job_for_migration(&engine1.registry.get(gid).expect("job"));
        native_row.sources = vec!["https://native.example/file.bin".into()];
        native_row.output_path = PathBuf::from("/tmp/native/file.bin");
        native_row.completed_bytes = 512;
        native_row.total_bytes = Some(2048);
        native_row.segments = 3;
        store.put_native_task(&native_row).unwrap();
        drop(engine1);

        let engine2 = Engine::with_store(default_config(), Arc::clone(&store));
        let count = engine2.restore().unwrap();

        assert_eq!(count, 1);
        let restored = engine2.registry.get(gid).expect("restored job");
        assert_eq!(restored.uris, vec!["https://native.example/file.bin"]);
        assert_eq!(restored.out_path, PathBuf::from("/tmp/native/file.bin"));
        assert_eq!(restored.downloaded, 512);
        assert_eq!(restored.total_size, Some(2048));
        assert_eq!(restored.options.max_connections, 3);
        let task_id = engine2.task_id_for_gid(gid).expect("task id");
        assert_eq!(task_id, native_row.task_id);
        assert_eq!(engine2.gid_for_task_id(&task_id), Some(gid));
    }

    #[test]
    fn engine_restore_demotes_active_to_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("crash.redb");
        let store = Arc::new(Store::open(&db_path).unwrap());

        // Phase 1: Add + activate (simulating a crash mid-download).
        let engine1 = Engine::with_store(default_config(), Arc::clone(&store));
        let handle = engine1.add_uri(&default_spec()).unwrap();
        engine1.activate_job(handle.gid).unwrap();
        let gid = handle.gid;
        drop(engine1); // Simulate crash.

        // Phase 2: Restore.
        let engine2 = Engine::with_store(default_config(), Arc::clone(&store));
        engine2.restore().unwrap();

        // Active job should be demoted to Waiting.
        let job = engine2.registry.get(gid).unwrap();
        assert_eq!(job.status, Status::Waiting);
        assert_eq!(engine2.scheduler.queue_len(), 1);
    }

    #[test]
    fn engine_restore_demotes_seeding_to_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("seeding.redb");
        let store = Arc::new(Store::open(&db_path).unwrap());

        let engine1 = Engine::with_store(default_config(), Arc::clone(&store));
        let handle = engine1.add_uri(&default_spec()).unwrap();
        engine1.activate_job(handle.gid).unwrap();
        engine1
            .registry
            .update(handle.gid, |job| job.status = Status::Seeding)
            .unwrap();
        store
            .put_job(&engine1.registry.get(handle.gid).unwrap())
            .unwrap();
        let gid = handle.gid;
        drop(engine1);

        let engine2 = Engine::with_store(default_config(), Arc::clone(&store));
        engine2.restore().unwrap();

        let job = engine2.registry.get(gid).unwrap();
        assert_eq!(job.status, Status::Waiting);
        assert_eq!(engine2.scheduler.queue_len(), 1);
    }

    #[test]
    fn engine_restore_keeps_completed_jobs_in_history() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.redb");
        let store = Arc::new(Store::open(&db_path).unwrap());

        let engine1 = Engine::with_store(default_config(), Arc::clone(&store));
        let handle = engine1.add_uri(&default_spec()).unwrap();
        engine1.activate_job(handle.gid).unwrap();
        engine1.complete_job(handle.gid).unwrap();
        let gid = handle.gid;
        drop(engine1);

        let engine2 = Engine::with_store(default_config(), Arc::clone(&store));
        engine2.restore().unwrap();

        let job = engine2.registry.get(gid).unwrap();
        assert_eq!(job.status, Status::Complete);
        // Completed jobs are NOT enqueued.
        assert_eq!(engine2.scheduler.queue_len(), 0);
    }

    #[test]
    fn engine_restore_keeps_paused_jobs_paused() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("paused.redb");
        let store = Arc::new(Store::open(&db_path).unwrap());

        let engine1 = Engine::with_store(default_config(), Arc::clone(&store));
        let handle = engine1.add_uri(&default_spec()).unwrap();
        engine1.activate_job(handle.gid).unwrap();
        engine1.pause(handle.gid).unwrap();
        let gid = handle.gid;
        drop(engine1);

        let engine2 = Engine::with_store(default_config(), Arc::clone(&store));
        engine2.restore().unwrap();

        let job = engine2.registry.get(gid).unwrap();
        assert_eq!(job.status, Status::Paused);
        // Paused jobs are NOT enqueued — wait for explicit unpause.
        assert_eq!(engine2.scheduler.queue_len(), 0);
    }

    #[test]
    fn engine_without_store_restore_fails() {
        let engine = Engine::new(default_config());
        let result = engine.restore();
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════════════════════════════
    // B2: CancelToken Wiring tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn activate_returns_cancel_token() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        let token = engine.activate_job(handle.gid).unwrap();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn pause_cancels_the_active_token() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        let token = engine.activate_job(handle.gid).unwrap();
        assert!(!token.is_cancelled());

        engine.pause(handle.gid).unwrap();
        // The token returned by activate_job is a CHILD of the job's root token.
        // When the root is cancelled, the child is too.
        assert!(token.is_cancelled());
    }

    #[test]
    fn unpause_creates_fresh_token() {
        let engine = Engine::new(default_config());
        let handle = engine.add_uri(&default_spec()).unwrap();

        let token1 = engine.activate_job(handle.gid).unwrap();
        engine.pause(handle.gid).unwrap();
        assert!(token1.is_cancelled());

        // Unpause creates a fresh root token via register().
        engine.unpause(handle.gid).unwrap();

        // Re-activate should give a new, non-cancelled token.
        let token2 = engine.activate_job(handle.gid).unwrap();
        assert!(!token2.is_cancelled());
    }

    // ═══════════════════════════════════════════════════════════════════
    // Batch operation tests (aria2 RPC parity)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn pause_all_pauses_active_and_waiting() {
        let engine = Engine::new(default_config());
        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();
        let _h3 = engine.add_uri(&default_spec()).unwrap();

        engine.activate_job(h1.gid).unwrap();
        engine.activate_job(h2.gid).unwrap();

        // h1, h2 = Active, h3 = Waiting.
        let paused = engine.pause_all();
        assert_eq!(paused, 3); // All 3 should be paused.

        assert_eq!(engine.registry.by_status(Status::Paused).len(), 3);
        assert_eq!(engine.registry.by_status(Status::Active).len(), 0);
        assert_eq!(engine.registry.by_status(Status::Waiting).len(), 0);
    }

    #[test]
    fn unpause_all_unpauses_only_paused() {
        let engine = Engine::new(default_config());
        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();
        let _h3 = engine.add_uri(&default_spec()).unwrap();

        engine.activate_job(h1.gid).unwrap();
        engine.activate_job(h2.gid).unwrap();

        // Pause 2 Active, leave h3 Waiting.
        engine.pause(h1.gid).unwrap();
        engine.pause(h2.gid).unwrap();

        // h1/h2 = Paused, h3 = Waiting.
        let unpaused = engine.unpause_all();
        assert_eq!(unpaused, 2);

        // Now h1, h2 = Waiting (again), h3 = Waiting.
        assert_eq!(engine.registry.by_status(Status::Waiting).len(), 3);
    }

    #[test]
    fn force_remove_works_on_active() {
        let engine = Engine::new(default_config());
        let h = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(h.gid).unwrap();

        // Normal remove on Active should work through state machine, but
        // force_remove bypasses it entirely.
        engine.force_remove(h.gid).unwrap();

        let job = engine.registry.get(h.gid).unwrap();
        assert_eq!(job.status, Status::Removed);
    }

    #[test]
    fn force_remove_works_on_waiting() {
        let engine = Engine::new(default_config());
        let h = engine.add_uri(&default_spec()).unwrap();

        engine.force_remove(h.gid).unwrap();
        let job = engine.registry.get(h.gid).unwrap();
        assert_eq!(job.status, Status::Removed);
    }

    #[test]
    fn force_remove_nonexistent_fails() {
        let engine = Engine::new(default_config());
        assert!(engine.force_remove(Gid::from_raw(999)).is_err());
    }

    #[test]
    fn remove_download_result_removes_completed() {
        let engine = Engine::new(default_config());
        let h = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(h.gid).unwrap();
        engine.complete_job(h.gid).unwrap();

        engine.remove_download_result(h.gid).unwrap();
        assert!(engine.registry.get(h.gid).is_none());
    }

    #[test]
    fn remove_download_result_removes_errored() {
        let engine = Engine::new(default_config());
        let h = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(h.gid).unwrap();
        engine.fail_job(h.gid, "oops").unwrap();

        engine.remove_download_result(h.gid).unwrap();
        assert!(engine.registry.get(h.gid).is_none());
    }

    #[test]
    fn remove_download_result_rejects_active() {
        let engine = Engine::new(default_config());
        let h = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(h.gid).unwrap();

        assert!(engine.remove_download_result(h.gid).is_err());
    }

    #[test]
    fn purge_download_results_purges_terminal_only() {
        let engine = Engine::new(default_config());
        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();
        let h3 = engine.add_uri(&default_spec()).unwrap();

        engine.activate_job(h1.gid).unwrap();
        engine.complete_job(h1.gid).unwrap(); // Complete.

        engine.activate_job(h2.gid).unwrap();
        engine.fail_job(h2.gid, "err").unwrap(); // Error.

        // h3 stays Waiting.

        let purged = engine.purge_download_results();
        assert_eq!(purged, 2);
        assert!(engine.registry.get(h1.gid).is_none());
        assert!(engine.registry.get(h2.gid).is_none());
        assert!(engine.registry.get(h3.gid).is_some()); // Waiting preserved.
    }

    #[test]
    fn change_position_set_moves_to_front() {
        let engine = Engine::new(default_config());
        let _h1 = engine.add_uri(&default_spec()).unwrap();
        let _h2 = engine.add_uri(&default_spec()).unwrap();
        let h3 = engine.add_uri(&default_spec()).unwrap();

        let new_pos = engine.change_position(h3.gid, 0, PositionHow::Set).unwrap();
        assert_eq!(new_pos, 0);

        let queue = engine.scheduler.waiting_queue();
        assert_eq!(queue[0], h3.gid);
    }

    #[test]
    fn change_position_cur_moves_relative() {
        let engine = Engine::new(default_config());
        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();
        let h3 = engine.add_uri(&default_spec()).unwrap();

        // h1 at pos 0, move +2 = pos 2.
        let new_pos = engine.change_position(h1.gid, 2, PositionHow::Cur).unwrap();
        assert_eq!(new_pos, 2);

        let queue = engine.scheduler.waiting_queue();
        assert_eq!(queue, vec![h2.gid, h3.gid, h1.gid]);
    }

    #[test]
    fn change_position_rejects_non_waiting() {
        let engine = Engine::new(default_config());
        let h = engine.add_uri(&default_spec()).unwrap();
        engine.activate_job(h.gid).unwrap();

        assert!(engine.change_position(h.gid, 0, PositionHow::Set).is_err());
    }

    #[test]
    fn save_session_persists_all_jobs() {
        let (engine, _dir) = engine_with_store();
        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();

        engine.save_session().unwrap();

        // Verify by reading from store directly.
        let store = engine.store.as_ref().unwrap();
        assert!(store.get_job(h1.gid).unwrap().is_some());
        assert!(store.get_job(h2.gid).unwrap().is_some());
    }

    #[test]
    fn save_session_persists_native_task_rows() {
        let (engine, _dir) = engine_with_store();
        let h1 = engine.add_uri(&default_spec()).unwrap();
        let h2 = engine.add_uri(&default_spec()).unwrap();
        engine.pause(h2.gid).unwrap();

        engine.save_session().unwrap();

        let store = engine.store.as_ref().unwrap();
        let rows = store.list_native_tasks().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| {
            row.task_id.as_str() == format!("task_migration_{:016x}", h1.gid.as_raw())
                && row.lifecycle == crate::native::TaskLifecycle::Queued
        }));
        assert!(rows.iter().any(|row| {
            row.task_id.as_str() == format!("task_migration_{:016x}", h2.gid.as_raw())
                && row.lifecycle == crate::native::TaskLifecycle::Paused
        }));
    }

    #[test]
    fn save_session_preserves_registered_native_task_ids() {
        let (engine, _dir) = engine_with_store();
        let handle = engine.add_uri(&default_spec()).unwrap();
        let task_id = TaskId::new();
        assert!(engine.register_native_task_id_for_migration(task_id.clone(), handle.gid));

        engine.save_session().unwrap();

        let store = engine.store.as_ref().unwrap();
        let row = store
            .get_native_task(&task_id)
            .unwrap()
            .expect("native task row");
        assert_eq!(row.task_id, task_id);
    }

    #[test]
    fn save_session_without_store_fails() {
        let engine = Engine::new(default_config());
        assert!(engine.save_session().is_err());
    }
    #[test]
    fn uptime_seconds_increases_over_time() {
        let engine = Engine::new(default_config());
        let t0 = engine.uptime_seconds();
        // uptime should be at least 0 (just created).
        assert!(t0 < 2, "fresh engine uptime should be near zero, got {t0}");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let t1 = engine.uptime_seconds();
        assert!(t1 >= t0, "uptime must not decrease");
    }
}
