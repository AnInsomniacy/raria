use crate::backend_factory::create_backend_with_config;
use crate::bt_runtime::{create_bt_service, run_bt_download};
use crate::executor_config::apply_global_retry_policy;
use crate::hooks::{HookConfig, spawn_hook_runner};
use crate::util::{build_conditional_get_probe_headers, parse_header_args, redact_url_for_logs};
use anyhow::{Context, Result};
use raria_core::checksum;
use raria_core::config::GlobalConfig;
use raria_core::engine::{
    AddUriSpec, Engine, NativeRangeExecutionTask, NativeSegmentCheckpointFn, NativeSegmentPlan,
    NativeSegmentPlanningInput,
};
use raria_core::input_file::InputFileEntry;
use raria_core::job::Gid;
use raria_core::native::{NativeEvent, NativeEventData, NativeEventType, TaskId};
use raria_core::persist::Store;
use raria_core::segment::SegmentStatus;
use raria_ed2k::link::{Ed2kFileLink, Ed2kLink, parse_link};
use raria_ed2k::peer::PeerIdentity;
use raria_ed2k::runtime::{
    Ed2kDiskRuntime, Ed2kDiskRuntimeConfig, Ed2kPeerDownloadRequest, Ed2kPeerRuntime,
    Ed2kPeerRuntimeConfig, Ed2kRuntimeConfig, Ed2kRuntimeContext, Ed2kRuntimeEventKind,
    Ed2kRuntimeStatus,
};
use raria_ed2k::source::SourceRecord;
use raria_range::backend::{ByteSourceBackend, Credentials, ProbeContext};
use raria_range::executor::{ExecutorConfig, SegmentExecutor, apply_results};
use raria_rpc::api::{NativeApiConfig, start_native_api_server};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub(crate) async fn run_daemon_with_config(
    config: GlobalConfig,
    session_file: &std::path::Path,
    input_entries: Vec<InputFileEntry>,
    download_dir: PathBuf,
    header_args: Vec<String>,
) -> Result<()> {
    let default_headers = parse_header_args(&header_args)?;
    let api_port = config.api_listen_port;

    std::fs::create_dir_all(&config.download_dir).context("failed to create download directory")?;

    let store = Arc::new(Store::open(session_file)?);
    let engine = Arc::new(Engine::with_store(config.clone(), Arc::clone(&store)));
    let bt_service = create_bt_service(engine.as_ref(), config.download_dir.clone())?;
    raria_core::logging::replace_structured_log_context([(
        "session_id",
        engine.session_id.clone(),
    )])?;

    let restored = engine.restore().unwrap_or_else(|e| {
        warn!(error = %e, "failed to restore jobs from session");
        0
    });
    if restored > 0 {
        info!(count = restored, "restored jobs from session");
    }

    for entry in &input_entries {
        let spec = AddUriSpec {
            uris: entry.uris.clone(),
            filename: entry.options.filename.clone(),
            dir: entry
                .options
                .download_dir
                .clone()
                .unwrap_or_else(|| download_dir.clone()),
            connections: entry
                .options
                .extra
                .get("segments")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(1),
            headers: Vec::new(),
            http_user: None,
            http_password: None,
            checksum: entry.options.checksum.clone(),
        };
        match engine.add_uri(&spec) {
            Ok(handle) => {
                let parse_headers = parse_header_args(&entry.options.headers);
                engine.registry.update(handle.gid, |job| {
                    if let Some(checksum) = entry.options.checksum.clone() {
                        job.options.checksum = Some(checksum);
                    }
                    if let Some(user) = entry.options.http_username.clone() {
                        job.options.http_user = Some(user);
                    }
                    if let Some(passwd) = entry.options.http_password.clone() {
                        job.options.http_password = Some(passwd);
                    }
                    if let Ok(headers) = &parse_headers {
                        job.options.headers.extend(headers.clone());
                    }
                    if let Some(limit) = entry
                        .options
                        .extra
                        .get("download-limit")
                        .and_then(|value| value.parse::<u64>().ok())
                    {
                        job.options.max_download_limit = limit;
                    }
                    if let Some(default_segments) = entry
                        .options
                        .extra
                        .get("segments")
                        .and_then(|value| value.parse::<u32>().ok())
                    {
                        job.options.max_connections = default_segments;
                    }
                });
                if let Err(error) = parse_headers {
                    warn!(
                        gid = %handle.gid,
                        error = %error,
                        "failed to parse task-file headers, continuing without them"
                    );
                }
                info!(gid = %handle.gid, "added job from task file");
            }
            Err(e) => warn!(uris = ?entry.uris, error = %e, "failed to add URI from task file"),
        }
    }

    let shutdown_token = engine.shutdown_token();
    let engine_for_ctrl_c = Arc::clone(&engine);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down daemon...");
        engine_for_ctrl_c.shutdown();
    });

    #[cfg(unix)]
    {
        let engine_ref = Arc::clone(&engine);
        let shutdown_ref = shutdown_token.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigusr1 = match signal(SignalKind::user_defined1()) {
                Ok(stream) => stream,
                Err(error) => {
                    warn!(error = %error, "failed to install SIGUSR1 handler");
                    return;
                }
            };

            while !shutdown_ref.is_cancelled() {
                sigusr1.recv().await;
                if shutdown_ref.is_cancelled() {
                    break;
                }
                match engine_ref.save_session() {
                    Ok(()) => info!("session saved by SIGUSR1"),
                    Err(error) => warn!(error = %error, "failed to save session on SIGUSR1"),
                }
            }
        });
    }

    #[cfg(unix)]
    {
        let engine_for_sigterm = Arc::clone(&engine);
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(stream) => stream,
                Err(error) => {
                    warn!(error = %error, "failed to install SIGTERM handler");
                    return;
                }
            };

            sigterm.recv().await;
            info!("received SIGTERM, shutting down daemon...");
            engine_for_sigterm.shutdown();
        });
    }

    if let Some(seconds) = config
        .daemon_stop_after_seconds
        .filter(|seconds| *seconds > 0)
    {
        let engine_ref = Arc::clone(&engine);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
            info!(seconds, "daemon stop timer elapsed");
            engine_ref.shutdown();
        });
    }

    if let Some(parent_pid) = config.daemon_parent_pid {
        let engine_ref = Arc::clone(&engine);
        tokio::spawn(async move {
            watch_parent_process(parent_pid, engine_ref).await;
        });
    }

    let api_cancel = CancellationToken::new();
    spawn_hook_runner(
        Arc::clone(&engine),
        HookConfig {
            on_task_start: config.on_task_start.clone(),
            on_task_complete: config.on_task_complete.clone(),
            on_task_fail: config.on_task_fail.clone(),
        },
        shutdown_token.clone(),
    );
    let api_config = NativeApiConfig {
        listen_addr: std::net::SocketAddr::from(([0, 0, 0, 0], api_port)),
        auth_token: config.api_auth_token.clone(),
    };
    let api_addrs =
        start_native_api_server(Arc::clone(&engine), &api_config, api_cancel.clone()).await?;
    info!(api = %api_addrs.http, "native API server ready");
    if !config.quiet {
        println!(
            "raria daemon running — API at http://{}/api/v1",
            api_addrs.http
        );
    }

    let work_notify = engine.work_notify();
    let session_save_interval = config
        .save_session_interval
        .filter(|interval| *interval > 0)
        .map(|interval| tokio::time::interval(std::time::Duration::from_secs(interval)));
    let mut session_save_interval = session_save_interval;

    loop {
        if shutdown_token.is_cancelled() {
            break;
        }

        let to_activate = engine.activatable_native_tasks();

        for task_id in to_activate {
            let activation = match engine.activate_native_task(&task_id) {
                Ok(activation) => activation,
                Err(e) => {
                    warn!(%task_id, error = %e, "failed to activate task");
                    continue;
                }
            };
            let token = activation.cancel;
            let task_id = activation.task_id.clone();

            let engine_ref = Arc::clone(&engine);

            match activation.kind {
                raria_core::job::JobKind::Range => {
                    let default_headers = default_headers.clone();
                    let range_context = RangeExecutionContext {
                        task_id: task_id.clone(),
                    };
                    let engine_for_failure = Arc::clone(&engine_ref);
                    tokio::spawn(async move {
                        if let Err(e) = run_job_download(
                            engine_ref,
                            range_context,
                            token,
                            default_headers.clone(),
                        )
                        .await
                        {
                            error!(%task_id, error = %e, "range download task failed");
                            let _ = engine_for_failure.fail_native_task(
                                &task_id,
                                &classified_error_message(&e.to_string()),
                            );
                        }
                    });
                }
                raria_core::job::JobKind::Bt => {
                    let bt_service = Arc::clone(&bt_service);
                    tokio::spawn(async move {
                        if let Err(e) =
                            run_bt_download(engine_ref, task_id.clone(), token, bt_service).await
                        {
                            error!(%task_id, error = %e, "BT download task failed");
                        }
                    });
                }
                raria_core::job::JobKind::Ed2k => {
                    tokio::spawn(async move {
                        if let Err(e) = run_ed2k_download(engine_ref, task_id.clone(), token).await
                        {
                            error!(%task_id, error = %e, "ED2K runtime task failed");
                        }
                    });
                }
            }
        }

        tokio::select! {
            _ = work_notify.notified() => {}
            _ = shutdown_token.cancelled() => { break; }
            _ = async {
                if let Some(interval) = &mut session_save_interval {
                    interval.tick().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                match engine.save_session() {
                    Ok(()) => info!("session saved by periodic interval"),
                    Err(e) => warn!(error = %e, "failed to save session on interval"),
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
        }
    }

    info!("daemon shutting down...");
    engine.cancel_active_native_tasks();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    bt_service.shutdown().await;
    match engine.save_session() {
        Ok(()) => info!("session saved successfully"),
        Err(e) => warn!(error = %e, "failed to save session on shutdown"),
    }

    api_cancel.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    info!("daemon stopped");
    Ok(())
}

#[cfg(unix)]
async fn watch_parent_process(parent_pid: u32, engine: Arc<Engine>) {
    loop {
        if engine.shutdown_token().is_cancelled() {
            break;
        }
        if !process_is_alive(parent_pid) {
            info!(parent_pid, "parent process exited, shutting down daemon");
            engine.shutdown();
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
async fn watch_parent_process(parent_pid: u32, engine: Arc<Engine>) {
    warn!(
        parent_pid,
        "parent process monitoring is not supported on this platform"
    );
    engine.shutdown();
}

/// Configuration context built from engine globals for a single download job.
struct DownloadContext {
    http_cfg: raria_http::backend::HttpBackendConfig,
    ftp_cfg: raria_ftp::backend::FtpBackendConfig,
    sftp_cfg: raria_sftp::backend::SftpBackendConfig,
    probe_ctx: ProbeContext,
    request_headers: Vec<(String, String)>,
    request_auth: Option<Credentials>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorClass {
    Transient,
    Permanent,
}

fn classify_error(message: &str) -> ErrorClass {
    let msg = message.to_ascii_lowercase();

    let permanent_markers = [
        "404",
        "not found",
        "checksum mismatch",
        "invalid uri",
        "invalid url",
        "unsupported",
        "permission denied",
        "unauthorized",
        "forbidden",
    ];
    if msg.contains("401")
        || msg.contains("unauthorized")
        || msg.contains("403")
        || msg.contains("forbidden")
    {
        return ErrorClass::Permanent;
    }

    if permanent_markers.iter().any(|marker| msg.contains(marker)) {
        return ErrorClass::Permanent;
    }

    let transient_markers = [
        "timeout",
        "timed out",
        "connection reset",
        "connection refused",
        "broken pipe",
        "temporarily unavailable",
        "temporary dns",
        "dns",
        "500",
        "502",
        "503",
        "504",
    ];
    if transient_markers.iter().any(|marker| msg.contains(marker)) {
        return ErrorClass::Transient;
    }

    ErrorClass::Transient
}

fn classified_error_message(message: &str) -> String {
    let class = match classify_error(message) {
        ErrorClass::Transient => "transient",
        ErrorClass::Permanent => "permanent",
    };
    format!("{class} error: {message}")
}

fn record_source_failure(engine: &Engine, gid: Gid, task_id: &TaskId, uri: &str, error_msg: &str) {
    let classified = classified_error_message(error_msg);
    if let Err(error) = engine.source_failed_native_task(task_id, uri, &classified) {
        warn!(
            %gid,
            task_id = %task_id,
            uri,
            error = %error,
            "failed to publish source-failed event"
        );
    }
}

fn record_source_success(
    engine: &Engine,
    gid: Gid,
    task_id: &TaskId,
    uri: &str,
    download_bytes_per_second: u64,
) {
    if let Err(error) = engine.source_succeeded_native_task(task_id, uri, download_bytes_per_second)
    {
        warn!(
            %gid,
            task_id = %task_id,
            uri,
            error = %error,
            "failed to record source health"
        );
    }
}

fn emit_integrity_failure_log(
    gid: Gid,
    task_id: &TaskId,
    uri: &str,
    error_msg: &str,
    cached: bool,
    retrying: bool,
) {
    let message = match (cached, retrying) {
        (true, true) => "cached mirror output failed verification, trying next mirror",
        (true, false) => "cached mirror output failed verification",
        (false, true) => "mirror payload failed verification, trying next mirror",
        (false, false) => "mirror payload failed verification",
    };
    raria_core::logging::emit_structured_log(
        "WARN",
        "raria::daemon",
        message,
        range_structured_fields(
            gid,
            task_id,
            [("uri", uri.to_string()), ("error", error_msg.to_string())],
        ),
    );
}

fn range_structured_fields(
    _gid: Gid,
    task_id: &TaskId,
    fields: impl IntoIterator<Item = (&'static str, String)>,
) -> Vec<(&'static str, String)> {
    let mut merged = vec![("task_id", task_id.to_string())];
    merged.extend(fields);
    merged
}

fn cleanup_segment_checkpoints(engine: &Engine, gid: Gid, task_id: &TaskId) {
    if let Err(e) = engine.cleanup_native_segment_checkpoints(task_id) {
        tracing::warn!(
            %gid,
            task_id = %task_id,
            error = %e,
            "failed to clean up native segment checkpoints"
        );
    }
}

/// Build protocol-specific backend configs and probe context from engine globals.
fn build_download_context(
    engine: &Engine,
    task: &NativeRangeExecutionTask,
    default_headers: &[(String, String)],
) -> DownloadContext {
    let mut request_headers: Vec<(String, String)> = default_headers.to_vec();
    request_headers.extend(task.request_headers.clone());
    let request_auth = task
        .http_user
        .as_ref()
        .map(|username| Credentials {
            username: username.clone(),
            password: task.http_password.clone().unwrap_or_default(),
        })
        .or_else(|| {
            engine
                .config
                .http_user
                .as_ref()
                .map(|username| Credentials {
                    username: username.clone(),
                    password: engine.config.http_password.clone().unwrap_or_default(),
                })
        });

    let http_cfg = raria_http::backend::HttpBackendConfig {
        proxy: engine.config.proxy.clone(),
        http_proxy: engine.config.http_proxy.clone(),
        https_proxy: engine.config.https_proxy.clone(),
        no_proxy: engine.config.no_proxy.clone(),
        check_certificate: engine.config.check_certificate,
        ca_certificate: engine.config.ca_certificate.clone(),
        client_certificate: engine.config.certificate.clone(),
        client_private_key: engine.config.private_key.clone(),
        user_agent: engine.config.user_agent.clone(),
        load_cookie_file: engine.config.load_cookie_file.clone(),
        cookie_store_file: engine.config.cookie_store_file.clone(),
        max_redirects: engine.config.max_redirects,
        connect_timeout: engine.config.connect_timeout,
        netrc_path: engine.config.netrc_path.clone(),
        no_netrc: engine.config.no_netrc,
    };
    let ftp_cfg = raria_ftp::backend::FtpBackendConfig {
        proxy: engine.config.proxy.clone(),
        no_proxy: engine.config.no_proxy.clone(),
        check_certificate: engine.config.check_certificate,
        ca_certificate: engine.config.ca_certificate.clone(),
    };
    let sftp_cfg = raria_sftp::backend::SftpBackendConfig {
        strict_host_key_check: engine.config.sftp_strict_host_key_check,
        known_hosts_path: engine.config.sftp_known_hosts.clone(),
        private_key_path: engine.config.sftp_private_key.clone(),
        private_key_passphrase: engine.config.sftp_private_key_passphrase.clone(),
        proxy: engine.config.proxy.clone(),
        no_proxy: engine.config.no_proxy.clone(),
    };
    let probe_ctx = ProbeContext {
        headers: request_headers.clone(),
        auth: request_auth.clone(),
        timeout: std::time::Duration::from_secs(engine.config.timeout.unwrap_or(30)),
    };

    DownloadContext {
        http_cfg,
        ftp_cfg,
        sftp_cfg,
        probe_ctx,
        request_headers,
        request_auth,
    }
}

/// Resolve the output file path, applying server-suggested filename if the user
/// did not explicitly set one via `--filename`.
fn resolve_output_path(
    engine: &Engine,
    gid: Gid,
    task: &NativeRangeExecutionTask,
    probe: &raria_range::backend::FileProbe,
) -> std::path::PathBuf {
    if !task.has_explicit_output_name {
        if let Some(filename) = probe.suggested_filename.clone() {
            match engine.set_native_output_filename_if_unset(&task.task_id, &filename) {
                Ok(path) => return path,
                Err(error) => {
                    warn!(%gid, task_id = %task.task_id, error = %error, "failed to apply native output filename");
                }
            }
        }
    }
    task.output_path.clone()
}

#[derive(Debug, Clone)]
struct RangeExecutionContext {
    task_id: TaskId,
}

/// Plan download segments and restore checkpoint progress from persistent store.
///
/// Returns `(connections, segments, checkpoint_callback)`.
fn plan_download_segments(
    engine: &Arc<Engine>,
    gid: Gid,
    task: &NativeRangeExecutionTask,
    source_uri: &str,
    probe: &raria_range::backend::FileProbe,
) -> (
    u32,
    Vec<raria_core::segment::SegmentState>,
    Option<NativeSegmentCheckpointFn>,
) {
    let plan = match engine.plan_native_segments(
        &task.task_id,
        NativeSegmentPlanningInput {
            total_size: probe.size,
            supports_range: probe.supports_range,
            requested_connections: task.max_connections,
            min_segment_size: engine.config.min_segment_size,
            source_uri: Some(source_uri),
        },
    ) {
        Ok(plan) => plan,
        Err(error) => {
            warn!(%gid, task_id = %task.task_id, error = %error, "failed to plan native segments, using single fresh segment");
            NativeSegmentPlan {
                connections: 1,
                segments: vec![raria_core::segment::SegmentState {
                    start: 0,
                    end: probe.size.unwrap_or(u64::MAX),
                    downloaded: 0,
                    etag: None,
                    status: SegmentStatus::Pending,
                }],
            }
        }
    };
    for (seg_id, segment) in plan.segments.iter().enumerate() {
        if segment.downloaded > 0 {
            info!(
                %gid, seg_id, resumed = segment.downloaded,
                "resumed segment from checkpoint"
            );
        }
    }

    let on_checkpoint = engine.native_segment_checkpoint_callback(&task.task_id, &plan.segments);

    (plan.connections, plan.segments, on_checkpoint)
}

fn verification_failure_message(
    piece_checksum: Option<&raria_core::job::PieceChecksum>,
    error: &anyhow::Error,
) -> String {
    if piece_checksum.is_some() {
        format!("piece checksum verification failed: {error}")
    } else {
        format!("checksum verification failed: {error}")
    }
}

/// Verify integrity for a fully downloaded file before marking it complete.
async fn verify_download_integrity(
    gid: Gid,
    out_path: &std::path::Path,
    piece_checksum: Option<&raria_core::job::PieceChecksum>,
    checksum_spec: Option<&str>,
) -> Result<()> {
    if let Some(piece_checksum) = piece_checksum {
        info!(%gid, "verifying piece checksums...");
        checksum::verify_piece_checksums(out_path, piece_checksum)
            .await
            .map_err(|error| {
                anyhow::anyhow!(verification_failure_message(Some(piece_checksum), &error))
            })?;
        info!(%gid, "piece checksums verified successfully");
    }
    if let Some(spec) = checksum_spec {
        info!(%gid, "verifying checksum...");
        checksum::verify_checksum(out_path, spec)
            .await
            .map_err(|error| anyhow::anyhow!(verification_failure_message(None, &error)))?;
        info!(%gid, "checksum verified successfully");
    }
    Ok(())
}

/// Finalize a completed download: update registry, clean up checkpoints, log.
async fn finalize_complete(
    engine: &Engine,
    gid: Gid,
    task_id: &TaskId,
    downloaded: u64,
) -> Result<()> {
    engine.complete_native_task(task_id, downloaded)?;
    cleanup_segment_checkpoints(engine, gid, task_id);
    info!(%gid, task_id = %task_id, bytes = downloaded, "daemon: download complete");
    Ok(())
}

fn reset_for_next_mirror(
    engine: &Engine,
    gid: Gid,
    task_id: &TaskId,
    out_path: &std::path::Path,
    segments: Option<&mut Vec<raria_core::segment::SegmentState>>,
) {
    cleanup_segment_checkpoints(engine, gid, task_id);
    if let Err(error) = engine.reset_native_task_for_next_source(task_id) {
        warn!(%gid, task_id = %task_id, error = %error, "failed to reset native task for next source");
    }
    if let Some(segments) = segments {
        for segment in segments.iter_mut() {
            segment.downloaded = 0;
            segment.status = SegmentStatus::Pending;
        }
    }
    if let Err(error) = std::fs::remove_file(out_path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(%gid, path = %out_path.display(), error = %error, "failed to remove corrupt mirror output before retry");
        }
    }
}

fn has_unattempted_registered_uri(
    engine: &Engine,
    task_id: &TaskId,
    attempted_counts: &HashMap<String, usize>,
) -> Result<bool> {
    Ok(engine
        .native_task_next_source(task_id, attempted_counts)?
        .is_some())
}

/// Persist interrupted segment state for future resumption.
fn persist_interrupted_segments(
    engine: &Engine,
    gid: Gid,
    task_id: &TaskId,
    segments: &[raria_core::segment::SegmentState],
    downloaded: u64,
) {
    if let Err(e) = engine.persist_native_interrupted_segments(task_id, segments, downloaded) {
        tracing::warn!(
            %gid,
            task_id = %task_id,
            error = %e,
            "failed to persist interrupted native segment state"
        );
    }
    info!(%gid, downloaded, "daemon: download interrupted");
}

async fn run_job_download(
    engine: Arc<Engine>,
    context: RangeExecutionContext,
    cancel: CancellationToken,
    default_headers: Vec<(String, String)>,
) -> Result<()> {
    let gid = engine
        .gid_for_task_id(&context.task_id)
        .context("native task runtime bridge missing")?;
    let task = engine
        .native_range_execution_task(&context.task_id)
        .context("failed to build native range execution task")?;
    let rate_limiter = Some(
        engine
            .native_task_rate_limiter(&context.task_id, task.max_download_limit)
            .context("failed to get native task rate limiter")?,
    );

    let ctx = build_download_context(&engine, &task, &default_headers);

    let engine_ref = Arc::clone(&engine);
    let task_id_for_progress = context.task_id.clone();
    let on_progress: Arc<dyn Fn(u32, u64) + Send + Sync> = Arc::new(move |_seg_id, bytes| {
        if let Err(error) = engine_ref.update_native_progress(&task_id_for_progress, bytes) {
            warn!(%gid, error = %error, "failed to update native task progress");
        }
    });

    let mut out_path: Option<std::path::PathBuf> = None;
    let mut effective_connections: Option<u32> = None;
    let mut segments: Option<Vec<raria_core::segment::SegmentState>> = None;
    let mut on_checkpoint: Option<NativeSegmentCheckpointFn> = None;
    let mut last_error: Option<String> = None;

    let mut attempted_counts: HashMap<String, usize> = HashMap::new();
    loop {
        let Some(uri_str) = engine
            .native_task_next_source(&context.task_id, &attempted_counts)
            .context("job not found in registry during mirror loop")?
        else {
            break;
        };
        *attempted_counts.entry(uri_str.clone()).or_insert(0) += 1;
        let parsed_url: url::Url = uri_str.parse().context("invalid URI")?;
        let redacted_url = redact_url_for_logs(parsed_url.as_str());
        info!(%gid, uri = %redacted_url, "daemon: starting download");
        raria_core::logging::emit_structured_log(
            "INFO",
            "raria::daemon",
            "daemon: starting download",
            range_structured_fields(gid, &context.task_id, [("uri", redacted_url.clone())]),
        );

        let backend = match create_backend_with_config(
            &uri_str,
            Some(&ctx.http_cfg),
            Some(&ctx.ftp_cfg),
            Some(&ctx.sftp_cfg),
        ) {
            Ok(backend) => backend,
            Err(error) => {
                warn!(%gid, uri = %redacted_url, error = %error, "failed to create backend for mirror");
                if has_unattempted_registered_uri(&engine, &context.task_id, &attempted_counts)? {
                    record_source_failure(
                        &engine,
                        gid,
                        &context.task_id,
                        &redacted_url,
                        &error.to_string(),
                    );
                }
                last_error = Some(classified_error_message(&error.to_string()));
                continue;
            }
        };

        let candidate_path = out_path.clone().unwrap_or_else(|| task.output_path.clone());
        let probe_headers = build_conditional_get_probe_headers(
            &engine.config,
            &parsed_url,
            &candidate_path,
            &ctx.request_headers,
        )?;
        let probe_ctx = ProbeContext {
            headers: probe_headers,
            auth: ctx.probe_ctx.auth.clone(),
            timeout: ctx.probe_ctx.timeout,
        };
        let probe = match backend.probe(&parsed_url, &probe_ctx).await {
            Ok(probe) => probe,
            Err(error) => {
                warn!(%gid, uri = %redacted_url, error = %error, "failed to probe mirror");
                if has_unattempted_registered_uri(&engine, &context.task_id, &attempted_counts)? {
                    record_source_failure(
                        &engine,
                        gid,
                        &context.task_id,
                        &redacted_url,
                        &error.to_string(),
                    );
                }
                last_error = Some(classified_error_message(&error.to_string()));
                continue;
            }
        };

        if out_path.is_none() {
            out_path = Some(resolve_output_path(&engine, gid, &task, &probe));
        }

        let out_path_ref = out_path.as_ref().expect("out_path initialized");
        if probe.not_modified {
            let existing_len = std::fs::metadata(out_path_ref)
                .map(|meta| meta.len())
                .unwrap_or(0);
            if let Err(error) = verify_download_integrity(
                gid,
                out_path_ref,
                task.piece_checksum.as_ref(),
                task.checksum.as_deref(),
            )
            .await
            {
                last_error = Some(classified_error_message(&error.to_string()));
                if has_unattempted_registered_uri(&engine, &context.task_id, &attempted_counts)? {
                    warn!(%gid, uri = %redacted_url, error = %error, "cached mirror output failed verification, trying next mirror");
                    record_source_failure(
                        &engine,
                        gid,
                        &context.task_id,
                        &redacted_url,
                        &error.to_string(),
                    );
                    emit_integrity_failure_log(
                        gid,
                        &context.task_id,
                        &redacted_url,
                        &error.to_string(),
                        true,
                        true,
                    );
                    reset_for_next_mirror(&engine, gid, &context.task_id, out_path_ref, None);
                    continue;
                }
                emit_integrity_failure_log(
                    gid,
                    &context.task_id,
                    &redacted_url,
                    &error.to_string(),
                    true,
                    false,
                );
                reset_for_next_mirror(&engine, gid, &context.task_id, out_path_ref, None);
                break;
            }
            record_source_success(&engine, gid, &context.task_id, &redacted_url, 0);
            return finalize_complete(&engine, gid, &context.task_id, existing_len).await;
        }

        if segments.is_none() {
            let (conns, segs, ckpt) = plan_download_segments(&engine, gid, &task, &uri_str, &probe);
            effective_connections = Some(conns);
            segments = Some(segs);
            on_checkpoint = ckpt;
        }

        let executor_cfg = apply_global_retry_policy(
            ExecutorConfig {
                max_connections: effective_connections.expect("connections initialized"),
                rate_limiter: rate_limiter.clone(),
                on_checkpoint: on_checkpoint.clone(),
                file_allocation: engine.config.file_allocation,
                request_timeout: std::time::Duration::from_secs(
                    engine.config.timeout.unwrap_or(60),
                ),
                request_headers: ctx.request_headers.clone(),
                request_auth: ctx.request_auth.clone(),
                request_etag: probe.etag.clone(),
                ..Default::default()
            },
            &engine.config,
        );
        let executor = SegmentExecutor::new(executor_cfg);

        let results = executor
            .execute(
                backend as Arc<dyn ByteSourceBackend>,
                &parsed_url,
                out_path.as_ref().expect("out_path initialized"),
                segments.as_ref().expect("segments initialized"),
                cancel.clone(),
                on_progress.clone(),
            )
            .await?;

        let segments_mut = segments.as_mut().expect("segments initialized");
        apply_results(segments_mut, &results);
        let downloaded_total: u64 = segments_mut.iter().map(|seg| seg.downloaded).sum();
        let all_done = results.iter().all(|r| r.status == SegmentStatus::Done);
        let failed: Vec<_> = results
            .iter()
            .filter(|r| r.status == SegmentStatus::Failed)
            .collect();

        if all_done {
            let out_path_ref = out_path.as_ref().expect("out_path initialized");
            if let Err(error) = verify_download_integrity(
                gid,
                out_path_ref,
                task.piece_checksum.as_ref(),
                task.checksum.as_deref(),
            )
            .await
            {
                last_error = Some(classified_error_message(&error.to_string()));
                if has_unattempted_registered_uri(&engine, &context.task_id, &attempted_counts)? {
                    warn!(%gid, uri = %redacted_url, error = %error, "mirror payload failed verification, trying next mirror");
                    record_source_failure(
                        &engine,
                        gid,
                        &context.task_id,
                        &redacted_url,
                        &error.to_string(),
                    );
                    emit_integrity_failure_log(
                        gid,
                        &context.task_id,
                        &redacted_url,
                        &error.to_string(),
                        false,
                        true,
                    );
                    reset_for_next_mirror(
                        &engine,
                        gid,
                        &context.task_id,
                        out_path_ref,
                        Some(segments_mut),
                    );
                    continue;
                }
                emit_integrity_failure_log(
                    gid,
                    &context.task_id,
                    &redacted_url,
                    &error.to_string(),
                    false,
                    false,
                );
                reset_for_next_mirror(
                    &engine,
                    gid,
                    &context.task_id,
                    out_path_ref,
                    Some(segments_mut),
                );
                break;
            }
            record_source_success(
                &engine,
                gid,
                &context.task_id,
                &redacted_url,
                engine
                    .native_task_summary(&context.task_id)
                    .map(|summary| summary.download_bytes_per_second)
                    .unwrap_or_default(),
            );
            return finalize_complete(&engine, gid, &context.task_id, downloaded_total).await;
        }

        if !failed.is_empty() {
            let raw_err_msg = failed
                .iter()
                .map(|r| {
                    format!(
                        "segment {}: {}",
                        r.segment_id,
                        r.error.as_deref().unwrap_or("unknown")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            last_error = Some(classified_error_message(&raw_err_msg));
            if has_unattempted_registered_uri(&engine, &context.task_id, &attempted_counts)? {
                warn!(%gid, uri = %redacted_url, "mirror failed, trying next mirror");
                record_source_failure(&engine, gid, &context.task_id, &redacted_url, &raw_err_msg);
                raria_core::logging::emit_structured_log(
                    "WARN",
                    "raria::daemon",
                    "mirror failed, trying next mirror",
                    range_structured_fields(gid, &context.task_id, [("uri", redacted_url.clone())]),
                );
                cleanup_segment_checkpoints(&engine, gid, &context.task_id);
                segments = None;
                effective_connections = None;
                on_checkpoint = None;
                continue;
            }

            engine.fail_native_task(
                &context.task_id,
                last_error
                    .as_deref()
                    .unwrap_or("transient error: mirror failed"),
            )?;
            return Ok(());
        }

        persist_interrupted_segments(
            &engine,
            gid,
            &context.task_id,
            segments_mut,
            downloaded_total,
        );
        return Ok(());
    }

    engine.fail_native_task(
        &context.task_id,
        last_error
            .as_deref()
            .unwrap_or("transient error: all mirrors failed"),
    )?;
    Ok(())
}

async fn run_ed2k_download(
    engine: Arc<Engine>,
    task_id: TaskId,
    cancel: CancellationToken,
) -> Result<()> {
    let mut context = Ed2kRuntimeContext::new(
        task_id.clone(),
        Ed2kRuntimeConfig::from_global_config(&engine.config),
    );
    for status in context.startup_statuses() {
        publish_ed2k_runtime_status(&engine, &task_id, status);
    }

    if let Some(file) = ed2k_file_from_task(&engine, &task_id)? {
        engine.set_native_segment_plan_metadata(&task_id, Some(file.size), 1)?;
        if !file.sources.is_empty() {
            run_ed2k_inline_peer_download(&engine, &task_id, &file).await?;
            if engine.native_task_summary(&task_id)?.lifecycle.as_str() == "completed" {
                return Ok(());
            }
        }
    }

    let started_at = std::time::Instant::now();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                for status in context.tick(started_at.elapsed()) {
                    publish_ed2k_runtime_status(&engine, &task_id, status);
                }
            }
        }
    }
    Ok(())
}

fn ed2k_file_from_task(engine: &Engine, task_id: &TaskId) -> Result<Option<Ed2kFileLink>> {
    let gid = engine
        .gid_for_task_id(task_id)
        .context("native ED2K task not found")?;
    let Some(job) = engine.registry.get(gid) else {
        return Ok(None);
    };
    for uri in &job.uris {
        if let Ed2kLink::File(file) =
            parse_link(uri).with_context(|| format!("invalid ED2K file link for {task_id}"))?
        {
            return Ok(Some(file));
        }
    }
    Ok(None)
}

async fn run_ed2k_inline_peer_download(
    engine: &Engine,
    task_id: &TaskId,
    file: &Ed2kFileLink,
) -> Result<()> {
    let gid = engine
        .gid_for_task_id(task_id)
        .context("native ED2K task not found")?;
    let Some(job) = engine.registry.get(gid) else {
        return Ok(());
    };
    let aich_root = file
        .aich_root
        .as_deref()
        .map(raria_ed2k::hash::parse_aich_root_base32)
        .transpose()
        .context("invalid ED2K AICH root")?;
    let mut disk = Ed2kDiskRuntime::new(Ed2kDiskRuntimeConfig {
        path: job.out_path.clone(),
        name: file.name.clone(),
        file_size: file.size,
        root_hash: file.root_hash,
        part_hashes: file.part_hashes.clone(),
        aich_root,
        sharing_enabled: engine.config.ed2k_share_completed,
        upload_slots: engine.config.ed2k_max_upload_slots as usize,
        upload_waiting: usize::from(engine.config.ed2k_max_upload_slots).saturating_mul(16),
        now_seconds: chrono::Utc::now().timestamp().max(0) as u64,
    })?;
    let mut peer_runtime = Ed2kPeerRuntime::new(Ed2kPeerRuntimeConfig {
        local_identity: ed2k_local_peer_identity(&engine.config),
        io_timeout: Duration::from_secs(5),
        ..Default::default()
    });
    let mut completed_ranges = disk.verified_ranges().to_vec();
    let mut retained_sources = Vec::<SourceRecord>::new();

    for source in &file.sources {
        engine.set_native_runtime_connections(task_id, 1)?;
        let report = peer_runtime
            .download_once(Ed2kPeerDownloadRequest {
                endpoint_host: source.host.clone(),
                endpoint_port: source.port,
                file_hash: file.root_hash,
                file_size: file.size,
                local_part_status: vec![false; file.part_hashes.len().max(1)],
                completed_ranges: completed_ranges.clone(),
                globally_requested: Vec::new(),
                hashset_required: !file.part_hashes.is_empty(),
                max_new_ranges: 1,
                request_source_exchange: true,
                now_seconds: chrono::Utc::now().timestamp().max(0) as u64,
            })
            .await?;
        retained_sources.extend(report.sources);
        for part in report.received_parts {
            let bytes = part.data.len() as u64;
            let disk_report = disk.apply_part(part)?;
            engine.update_native_progress(task_id, bytes)?;
            completed_ranges = disk_report.verified_ranges;
            if disk_report.completed {
                engine.complete_native_task(task_id, file.size)?;
                engine.set_native_runtime_connections(task_id, 0)?;
                publish_ed2k_status(
                    engine,
                    task_id,
                    NativeEventType::TaskEd2kTransferUpdated,
                    "transfer",
                    "completed",
                    Some("ED2K inline peer transfer completed"),
                    BTreeMap::from([("knownSources".to_string(), retained_sources.len() as u64)]),
                );
                return Ok(());
            }
        }
    }
    engine.set_native_runtime_connections(task_id, 0)?;
    Ok(())
}

fn ed2k_local_peer_identity(config: &GlobalConfig) -> PeerIdentity {
    PeerIdentity {
        user_hash: [0x11; 16],
        client_id: 0,
        tcp_port: config.ed2k_listen_tcp_port,
        udp_port: config.ed2k_listen_udp_port,
        kad_udp_port: config.ed2k_listen_udp_port,
        server: None,
        name: "raria".to_string(),
    }
}

fn publish_ed2k_runtime_status(engine: &Engine, task_id: &TaskId, status: Ed2kRuntimeStatus) {
    publish_ed2k_status(
        engine,
        task_id,
        ed2k_runtime_event_type(status.event_kind),
        status.category,
        status.state,
        status.message,
        status.metrics,
    );
}

fn ed2k_runtime_event_type(kind: Ed2kRuntimeEventKind) -> NativeEventType {
    match kind {
        Ed2kRuntimeEventKind::Source => NativeEventType::TaskEd2kSourceUpdated,
        Ed2kRuntimeEventKind::Queue => NativeEventType::TaskEd2kQueueUpdated,
        Ed2kRuntimeEventKind::Kad => NativeEventType::TaskEd2kKadUpdated,
        Ed2kRuntimeEventKind::Transfer => NativeEventType::TaskEd2kTransferUpdated,
        Ed2kRuntimeEventKind::Sharing => NativeEventType::TaskEd2kSharingUpdated,
        Ed2kRuntimeEventKind::Upload => NativeEventType::TaskEd2kUploadUpdated,
    }
}

fn publish_ed2k_status(
    engine: &Engine,
    task_id: &TaskId,
    event_type: NativeEventType,
    category: &str,
    state: &str,
    message: Option<&str>,
    metrics: std::collections::BTreeMap<String, u64>,
) {
    engine.native_event_bus.publish(NativeEvent::new(
        0,
        event_type,
        Some(task_id.clone()),
        NativeEventData::Ed2kStatus {
            category: category.to_string(),
            state: state.to_string(),
            message: message.map(str::to_string),
            metrics,
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use raria_core::job::Status;
    use raria_core::progress::DownloadEvent;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn heuristic_classifies_transient_errors() {
        assert_eq!(classify_error("operation timed out"), ErrorClass::Transient);
        assert_eq!(
            classify_error("connection reset by peer"),
            ErrorClass::Transient
        );
        assert_eq!(
            classify_error("temporary dns resolution failure"),
            ErrorClass::Transient
        );
    }

    #[test]
    fn heuristic_classifies_permanent_errors() {
        assert_eq!(
            classify_error("http status 404 not found"),
            ErrorClass::Permanent
        );
        assert_eq!(
            classify_error("http status 401 unauthorized"),
            ErrorClass::Permanent
        );
        assert_eq!(
            classify_error("http status 403 forbidden"),
            ErrorClass::Permanent
        );
        assert_eq!(
            classify_error("checksum mismatch for /tmp/file.bin"),
            ErrorClass::Permanent
        );
        assert_eq!(classify_error("invalid URI"), ErrorClass::Permanent);
    }

    #[test]
    fn prefixes_error_messages_with_classification() {
        assert_eq!(
            classified_error_message("operation timed out"),
            "transient error: operation timed out"
        );
        assert_eq!(
            classified_error_message("http status 404 not found"),
            "permanent error: http status 404 not found"
        );
    }

    #[test]
    fn daemon_classification_matches_core_service_heuristics() {
        use raria_core::service::{DownloadErrorClass, classify_download_error};

        for (message, expected) in [
            ("operation timed out", ErrorClass::Transient),
            ("http status 404 not found", ErrorClass::Permanent),
            ("unauthorized", ErrorClass::Permanent),
            ("forbidden", ErrorClass::Permanent),
        ] {
            let shared = match classify_download_error(message) {
                DownloadErrorClass::Transient => ErrorClass::Transient,
                DownloadErrorClass::Permanent => ErrorClass::Permanent,
            };
            assert_eq!(shared, expected, "shared classifier drifted for {message}");
            assert_eq!(
                classify_error(message),
                shared,
                "daemon classifier drifted from shared service classifier for {message}"
            );
        }
    }

    #[test]
    fn range_structured_fields_use_native_task_id() {
        let gid = Gid::from_raw(42);
        let task_id = TaskId::new();

        let fields = range_structured_fields(
            gid,
            &task_id,
            [("uri", "https://example.test/file.bin".to_string())],
        );

        assert!(fields.contains(&("task_id", task_id.to_string())));
        assert!(fields.contains(&("uri", "https://example.test/file.bin".to_string())));
        assert!(!fields.iter().any(|(key, _)| *key == "gid"));
    }

    #[test]
    fn interrupted_segment_persistence_uses_native_rows() {
        let dir = tempdir().expect("tempdir");
        let store_path = dir.path().join("session.redb");
        let store = Arc::new(Store::open(&store_path).expect("store"));
        let engine = Arc::new(Engine::with_store(
            GlobalConfig::default(),
            Arc::clone(&store),
        ));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.test/file.bin".to_string()],
                dir: dir.path().to_path_buf(),
                filename: Some("file.bin".to_string()),
                connections: 2,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .expect("add uri");
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        let segments = vec![raria_core::segment::SegmentState {
            start: 0,
            end: 1024,
            downloaded: 512,
            etag: None,
            status: SegmentStatus::Active,
        }];

        persist_interrupted_segments(&engine, handle.gid, &task_id, &segments, 512);

        assert_eq!(
            store
                .list_native_segments(&task_id)
                .expect("native segments")[0]
                .1
                .downloaded,
            512
        );
    }

    #[test]
    fn native_segment_rows_are_read_for_resume() {
        let dir = tempdir().expect("tempdir");
        let store_path = dir.path().join("session.redb");
        let store = Arc::new(Store::open(&store_path).expect("store"));
        let engine = Arc::new(Engine::with_store(
            GlobalConfig::default(),
            Arc::clone(&store),
        ));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.test/file.bin".to_string()],
                dir: dir.path().to_path_buf(),
                filename: Some("file.bin".to_string()),
                connections: 2,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .expect("add uri");
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        let task = engine
            .native_range_execution_task(&task_id)
            .expect("range task");
        let native_segment = raria_core::segment::SegmentState {
            start: 0,
            end: 2048,
            downloaded: 1024,
            etag: None,
            status: SegmentStatus::Active,
        };
        store
            .put_native_segment(&task_id, 0, &native_segment)
            .expect("native segment");
        let probe = raria_range::backend::FileProbe {
            size: Some(4096),
            supports_range: true,
            etag: None,
            last_modified: None,
            content_type: None,
            suggested_filename: None,
            not_modified: false,
        };

        let (_connections, segments, _checkpoint) = plan_download_segments(
            &engine,
            handle.gid,
            &task,
            "https://example.com/file.zip",
            &probe,
        );

        assert_eq!(segments[0].downloaded, 1024);
        assert_eq!(segments[0].status, SegmentStatus::Pending);
    }

    #[tokio::test]
    async fn finalize_complete_uses_native_task_id_as_terminal_authority() {
        let dir = tempdir().expect("tempdir");
        let store_path = dir.path().join("session.redb");
        let store = Arc::new(Store::open(&store_path).expect("store"));
        let engine = Engine::with_store(GlobalConfig::default(), Arc::clone(&store));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.test/file.bin".to_string()],
                dir: dir.path().to_path_buf(),
                filename: Some("file.bin".to_string()),
                connections: 1,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .expect("add uri");
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        engine.activate_job(handle.gid).expect("activate");

        finalize_complete(&engine, handle.gid, &task_id, 7)
            .await
            .expect("complete through native task id");

        let job = engine.registry.get(handle.gid).expect("job");
        assert_eq!(job.status, Status::Complete);
        assert_eq!(job.downloaded, 7);
    }

    #[tokio::test]
    async fn mirror_failover_publishes_source_failed_event_before_completion() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/ok.bin"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "2")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/ok.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok"))
            .mount(&server)
            .await;

        let dir = tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let spec = AddUriSpec {
            uris: vec![
                "gopher://example.invalid/file.bin".into(),
                format!("{}/ok.bin", server.uri()),
            ],
            dir: dir.path().to_path_buf(),
            filename: Some("ok.bin".into()),
            connections: 1,
            headers: Vec::new(),
            http_user: None,
            http_password: None,
            checksum: None,
        };
        let handle = engine.add_uri(&spec).expect("add uri");
        let mut rx = engine.event_bus.subscribe();
        let cancel = engine.activate_job(handle.gid).expect("activate job");

        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        run_job_download(
            Arc::clone(&engine),
            RangeExecutionContext { task_id },
            cancel,
            Vec::new(),
        )
        .await
        .expect("download should succeed after failover");

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(1);
        let mut saw_source_failed = false;
        let mut saw_complete = false;
        while !(saw_source_failed && saw_complete) {
            let event = tokio::time::timeout_at(deadline, rx.recv())
                .await
                .expect("timed out waiting for daemon events")
                .expect("daemon event stream should stay alive");

            match event {
                DownloadEvent::SourceFailed { gid, uri, message } => {
                    assert_eq!(gid, handle.gid);
                    assert_eq!(uri, "gopher://example.invalid/file.bin");
                    assert!(
                        message.starts_with("permanent error:"),
                        "expected classified mirror failure message, got {message}"
                    );
                    saw_source_failed = true;
                }
                DownloadEvent::Complete { gid } => {
                    assert_eq!(gid, handle.gid);
                    saw_complete = true;
                }
                _ => {}
            }
        }

        let job = engine.registry.get(handle.gid).expect("job");
        assert_eq!(job.status, Status::Complete);
        assert_eq!(
            std::fs::read(dir.path().join("ok.bin")).expect("downloaded output"),
            b"ok"
        );
    }

    #[tokio::test]
    async fn mirror_failover_replans_segments_for_selected_source_capabilities() {
        let primary = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/adaptive.bin"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "4096")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&primary)
            .await;
        Mock::given(method("GET"))
            .and(path("/adaptive.bin"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&primary)
            .await;

        let fallback = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/adaptive.bin"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-length", "4"))
            .mount(&fallback)
            .await;
        Mock::given(method("GET"))
            .and(path("/adaptive.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"done"))
            .expect(1)
            .mount(&fallback)
            .await;

        let dir = tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let spec = AddUriSpec {
            uris: vec![
                format!("{}/adaptive.bin", primary.uri()),
                format!("{}/adaptive.bin", fallback.uri()),
            ],
            dir: dir.path().to_path_buf(),
            filename: Some("adaptive.bin".into()),
            connections: 4,
            headers: Vec::new(),
            http_user: None,
            http_password: None,
            checksum: None,
        };
        let handle = engine.add_uri(&spec).expect("add uri");
        let cancel = engine.activate_job(handle.gid).expect("activate job");
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");

        run_job_download(
            Arc::clone(&engine),
            RangeExecutionContext { task_id },
            cancel,
            Vec::new(),
        )
        .await
        .expect("download should succeed after adaptive failover");

        let job = engine.registry.get(handle.gid).expect("job");
        assert_eq!(job.status, Status::Complete);
        assert_eq!(
            std::fs::read(dir.path().join("adaptive.bin")).expect("downloaded output"),
            b"done"
        );
    }

    #[tokio::test]
    async fn ed2k_runtime_waits_for_cancellation_without_failing_task() {
        let dir = tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(GlobalConfig::default()));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec![
                    "ed2k://|file|sample.iso|1234|0123456789abcdef0123456789abcdef|/".into(),
                ],
                dir: dir.path().to_path_buf(),
                filename: Some("sample.iso".into()),
                connections: 1,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .expect("add ED2K task");
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        let mut events = engine.native_event_bus.subscribe();
        let token = engine
            .activate_native_task(&task_id)
            .expect("activate")
            .cancel;

        let runtime = tokio::spawn(run_ed2k_download(
            Arc::clone(&engine),
            task_id.clone(),
            token.clone(),
        ));
        let started = events.recv().await.expect("started event");
        assert_eq!(
            started.event_type,
            raria_core::native::NativeEventType::TaskStarted
        );
        let source_event = events.recv().await.expect("ED2K source event");
        assert_eq!(
            source_event.event_type,
            raria_core::native::NativeEventType::TaskEd2kSourceUpdated
        );
        match source_event.data {
            NativeEventData::Ed2kStatus { state, message, .. } => {
                assert_eq!(state, "discovering");
                assert_eq!(
                    message.as_deref(),
                    Some("ED2K runtime scheduler initialized")
                );
            }
            other => panic!("unexpected ED2K source payload: {other:?}"),
        }
        let transfer_event = loop {
            let event = tokio::time::timeout(Duration::from_millis(1500), events.recv())
                .await
                .expect("runtime tick")
                .expect("ED2K runtime event");
            if event.event_type == raria_core::native::NativeEventType::TaskEd2kTransferUpdated {
                break event;
            }
        };
        assert_eq!(
            transfer_event.event_type,
            raria_core::native::NativeEventType::TaskEd2kTransferUpdated
        );

        token.cancel();
        runtime.await.expect("join").expect("runtime shutdown");
        let job = engine.registry.get(handle.gid).expect("job");
        assert_eq!(job.status, Status::Active);
        assert!(job.error_msg.is_none());
    }

    #[tokio::test]
    async fn ed2k_runtime_downloads_inline_peer_and_completes_task() {
        use raria_ed2k::hash::ed2k_root_hash;
        use raria_ed2k::opcode::PeerOpcode;
        use raria_ed2k::packet::{PacketFrame, Protocol, decode_tcp_frame, encode_tcp_frame};
        use raria_ed2k::peer::{
            PeerIdentity, build_emule_info, build_file_status_answer, build_peer_hello_answer,
        };
        use raria_ed2k::transfer::{PartRange, parse_part_request};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        const MAX_PACKET: usize = 16 * 1024;

        async fn read_frame(stream: &mut TcpStream) -> PacketFrame {
            let mut header = [0_u8; 6];
            stream.read_exact(&mut header).await.expect("header");
            let len = u32::from_le_bytes(header[1..5].try_into().expect("len")) as usize;
            let mut payload = vec![0_u8; len - 1];
            stream.read_exact(&mut payload).await.expect("payload");
            let mut raw = header.to_vec();
            raw.extend_from_slice(&payload);
            decode_tcp_frame(&raw, MAX_PACKET).expect("frame")
        }

        async fn write_frame(stream: &mut TcpStream, frame: &PacketFrame) {
            let bytes = encode_tcp_frame(frame, MAX_PACKET).expect("encode");
            stream.write_all(&bytes).await.expect("write");
        }

        fn hex_hash(hash: [u8; 16]) -> String {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = String::with_capacity(32);
            for byte in hash {
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0x0f) as usize] as char);
            }
            out
        }

        fn remote_identity() -> PeerIdentity {
            PeerIdentity {
                user_hash: [0x22; 16],
                client_id: 0x0506_0708,
                tcp_port: 4662,
                udp_port: 4672,
                kad_udp_port: 4672,
                server: None,
                name: "remote-test".to_string(),
            }
        }

        fn peer_frame(opcode: PeerOpcode, payload: Vec<u8>) -> PacketFrame {
            PacketFrame {
                protocol: Protocol::Edonkey,
                opcode: opcode.into(),
                payload,
            }
        }

        fn sending_part(file_hash: [u8; 16], range: PartRange, data: &[u8]) -> PacketFrame {
            let mut payload = file_hash.to_vec();
            payload.extend_from_slice(&(range.begin as u32).to_le_bytes());
            payload.extend_from_slice(&(range.end as u32).to_le_bytes());
            payload.extend_from_slice(data);
            peer_frame(PeerOpcode::SendingPart, payload)
        }

        let payload = b"data";
        let file_hash = ed2k_root_hash(payload);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let addr = listener.local_addr().expect("addr");
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            assert_eq!(
                read_frame(&mut stream).await.opcode,
                u8::from(PeerOpcode::Hello)
            );
            write_frame(
                &mut stream,
                &build_peer_hello_answer(&remote_identity()).expect("hello answer"),
            )
            .await;
            assert_eq!(read_frame(&mut stream).await.protocol, Protocol::Emule);
            write_frame(
                &mut stream,
                &build_emule_info(remote_identity().udp_port, true).expect("info answer"),
            )
            .await;
            assert_eq!(
                read_frame(&mut stream).await.opcode,
                u8::from(PeerOpcode::RequestSources)
            );
            write_frame(
                &mut stream,
                &raria_ed2k::source::build_source_exchange_answer(file_hash, 4, true, &[])
                    .expect("source exchange"),
            )
            .await;
            assert_eq!(
                read_frame(&mut stream).await.opcode,
                u8::from(PeerOpcode::SetRequestedFileId)
            );
            write_frame(
                &mut stream,
                &build_file_status_answer(file_hash, &[true]).expect("status"),
            )
            .await;
            assert_eq!(
                read_frame(&mut stream).await.opcode,
                u8::from(PeerOpcode::StartUploadRequest)
            );
            write_frame(
                &mut stream,
                &peer_frame(PeerOpcode::AcceptUploadRequest, Vec::new()),
            )
            .await;
            let part_request = read_frame(&mut stream).await;
            let ranges = parse_part_request(&part_request.payload, file_hash, false).expect("part");
            assert_eq!(ranges, vec![PartRange { begin: 0, end: 4 }]);
            write_frame(
                &mut stream,
                &sending_part(file_hash, PartRange { begin: 0, end: 4 }, payload),
            )
            .await;
        });

        let dir = tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(GlobalConfig {
            ed2k_enabled: true,
            ed2k_enable_servers: false,
            ed2k_enable_kad: false,
            ..Default::default()
        }));
        let link = format!(
            "ed2k://|file|sample.bin|4|{}|sources,{}:{}|/",
            hex_hash(file_hash),
            addr.ip(),
            addr.port()
        );
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec![link],
                dir: dir.path().to_path_buf(),
                filename: Some("sample.bin".into()),
                connections: 1,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .expect("add ED2K task");
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        let token = engine
            .activate_native_task(&task_id)
            .expect("activate")
            .cancel;

        tokio::time::timeout(
            Duration::from_secs(2),
            run_ed2k_download(Arc::clone(&engine), task_id, token),
        )
        .await
        .expect("runtime should complete")
        .expect("runtime result");
        peer.await.expect("peer");

        let job = engine.registry.get(handle.gid).expect("job");
        assert_eq!(job.status, Status::Complete);
        assert_eq!(job.downloaded, 4);
        assert_eq!(
            std::fs::read(dir.path().join("sample.bin")).expect("download"),
            payload
        );
    }

    #[test]
    fn plan_download_segments_uses_selected_source_health() {
        let dir = tempdir().expect("tempdir");
        let store_path = dir.path().join("session.redb");
        let store = Arc::new(Store::open(&store_path).expect("store"));
        let engine = Arc::new(Engine::with_store(
            GlobalConfig::default(),
            Arc::clone(&store),
        ));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://slow.example/file.bin".to_string()],
                dir: dir.path().to_path_buf(),
                filename: Some("file.bin".to_string()),
                connections: 4,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .expect("add uri");
        let task_id = engine.task_id_for_gid(handle.gid).expect("task id");
        engine
            .source_failed_native_task(
                &task_id,
                "https://slow.example/file.bin",
                "transient error: timeout",
            )
            .expect("record source failure");
        let task = engine
            .native_range_execution_task(&task_id)
            .expect("range task");
        let probe = raria_range::backend::FileProbe {
            size: Some(8192),
            supports_range: true,
            etag: None,
            last_modified: None,
            content_type: None,
            suggested_filename: None,
            not_modified: false,
        };

        let (connections, segments, _checkpoint) = plan_download_segments(
            &engine,
            handle.gid,
            &task,
            "https://slow.example/file.bin",
            &probe,
        );

        assert_eq!(connections, 2);
        assert_eq!(segments.len(), 2);
    }
}
