use crate::backend_factory::create_backend_with_config;
use crate::bt_runtime::{create_bt_service, run_bt_download};
use crate::executor_config::apply_global_retry_policy;
use crate::hooks::{HookConfig, spawn_hook_runner};
use crate::util::{build_conditional_get_probe_headers, parse_header_args, redact_url_for_logs};
use anyhow::{Context, Result};
use raria_core::checksum;
use raria_core::config::GlobalConfig;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::input_file::InputFileEntry;
use raria_core::job::Gid;
use raria_core::native::TaskId;
use raria_core::persist::Store;
use raria_core::segment::{SegmentStatus, init_segment_states, plan_segments};
use raria_range::backend::{ByteSourceBackend, Credentials, ProbeContext};
use raria_range::executor::{ExecutorConfig, SegmentExecutor, apply_results};
use raria_rpc::server::{RpcServerConfig, start_rpc_server};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
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
    let rpc_port = config.rpc_listen_port;

    std::fs::create_dir_all(&config.dir).context("failed to create download directory")?;

    let store = Arc::new(Store::open(session_file)?);
    let engine = Arc::new(Engine::with_store(config.clone(), Arc::clone(&store)));
    let bt_service = create_bt_service(engine.as_ref(), config.dir.clone())?;
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
            filename: entry.options.out.clone(),
            dir: entry
                .options
                .dir
                .clone()
                .unwrap_or_else(|| download_dir.clone()),
            connections: entry
                .options
                .extra
                .get("split")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(1),
        };
        match engine.add_uri(&spec) {
            Ok(handle) => {
                let parse_headers = parse_header_args(&entry.options.headers);
                engine.registry.update(handle.gid, |job| {
                    if let Some(checksum) = entry.options.checksum.clone() {
                        job.options.checksum = Some(checksum);
                    }
                    if let Some(user) = entry.options.http_user.clone() {
                        job.options.http_user = Some(user);
                    }
                    if let Some(passwd) = entry.options.http_passwd.clone() {
                        job.options.http_passwd = Some(passwd);
                    }
                    if let Ok(headers) = &parse_headers {
                        job.options.headers.extend(headers.clone());
                    }
                    if let Some(limit) = entry
                        .options
                        .extra
                        .get("max-download-limit")
                        .and_then(|value| value.parse::<u64>().ok())
                    {
                        job.options.max_download_limit = limit;
                    }
                    if let Some(split) = entry
                        .options
                        .extra
                        .get("split")
                        .and_then(|value| value.parse::<u32>().ok())
                    {
                        job.options.max_connections = split;
                    }
                });
                if let Err(error) = parse_headers {
                    warn!(
                        gid = %handle.gid,
                        error = %error,
                        "failed to parse input-file headers, continuing without them"
                    );
                }
                info!(gid = %handle.gid, "added job from input file");
            }
            Err(e) => warn!(uris = ?entry.uris, error = %e, "failed to add URI from input file"),
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

    let rpc_cancel = CancellationToken::new();
    spawn_hook_runner(
        Arc::clone(&engine),
        HookConfig {
            on_download_start: config.on_download_start.clone(),
            on_download_complete: config.on_download_complete.clone(),
            on_download_error: config.on_download_error.clone(),
        },
        shutdown_token.clone(),
    );
    let rpc_config = RpcServerConfig {
        listen_addr: std::net::SocketAddr::from(([0, 0, 0, 0], rpc_port)),
    };
    let rpc_addrs = start_rpc_server(Arc::clone(&engine), &rpc_config, rpc_cancel.clone()).await?;
    info!(rpc = %rpc_addrs.rpc, "RPC server ready");
    if !config.quiet {
        println!(
            "raria daemon running — API at http://{}/api/v1",
            rpc_addrs.rpc
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
            let gid = activation.runtime_gid;
            let token = activation.cancel;
            let range_context = RangeExecutionContext {
                task_id: activation.task_id.clone(),
                runtime_gid: gid,
            };

            let engine_ref = Arc::clone(&engine);

            match activation.kind {
                raria_core::job::JobKind::Range => {
                    let default_headers = default_headers.clone();
                    tokio::spawn(async move {
                        if let Err(e) = run_job_download(
                            engine_ref,
                            range_context,
                            token,
                            default_headers.clone(),
                        )
                        .await
                        {
                            error!(%gid, error = %e, "job download task failed");
                        }
                    });
                }
                raria_core::job::JobKind::Bt => {
                    let bt_service = Arc::clone(&bt_service);
                    tokio::spawn(async move {
                        if let Err(e) = run_bt_download(engine_ref, gid, token, bt_service).await {
                            error!(%gid, error = %e, "BT download task failed");
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

    rpc_cancel.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    info!("daemon stopped");
    Ok(())
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

fn record_source_failure(engine: &Engine, gid: Gid, uri: &str, error_msg: &str) {
    let classified = classified_error_message(error_msg);
    if let Err(error) = engine.source_failed(gid, uri, &classified) {
        warn!(
            %gid,
            uri,
            error = %error,
            "failed to publish source-failed event"
        );
    }
}

fn emit_integrity_failure_log(gid: Gid, uri: &str, error_msg: &str, cached: bool, retrying: bool) {
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
        [
            ("gid", gid.to_string()),
            ("uri", uri.to_string()),
            ("error", error_msg.to_string()),
        ],
    );
}

fn cleanup_segment_checkpoints(engine: &Engine, gid: Gid) {
    if let Some(store) = engine.store() {
        if let Some(job) = engine.registry.get(gid) {
            if let Err(e) = store.remove_native_segments(&job.task_id) {
                tracing::warn!(%gid, task_id = %job.task_id, error = %e, "failed to clean up native segment checkpoints");
            }
        }
        if let Err(e) = store.remove_segments(gid) {
            tracing::warn!(%gid, error = %e, "failed to clean up segment checkpoints");
        }
    }
}

/// Build protocol-specific backend configs and probe context from engine globals.
fn build_download_context(
    engine: &Engine,
    job: &raria_core::job::Job,
    default_headers: &[(String, String)],
) -> DownloadContext {
    let mut request_headers: Vec<(String, String)> = default_headers.to_vec();
    request_headers.extend(job.options.headers.clone());
    let request_auth = job
        .options
        .http_user
        .as_ref()
        .map(|username| Credentials {
            username: username.clone(),
            password: job.options.http_passwd.clone().unwrap_or_default(),
        })
        .or_else(|| {
            engine
                .config
                .http_user
                .as_ref()
                .map(|username| Credentials {
                    username: username.clone(),
                    password: engine.config.http_passwd.clone().unwrap_or_default(),
                })
        });

    let http_cfg = raria_http::backend::HttpBackendConfig {
        all_proxy: engine.config.all_proxy.clone(),
        http_proxy: engine.config.http_proxy.clone(),
        https_proxy: engine.config.https_proxy.clone(),
        no_proxy: engine.config.no_proxy.clone(),
        check_certificate: engine.config.check_certificate,
        ca_certificate: engine.config.ca_certificate.clone(),
        client_certificate: engine.config.certificate.clone(),
        client_private_key: engine.config.private_key.clone(),
        user_agent: engine.config.user_agent.clone(),
        cookie_file: engine.config.cookie_file.clone(),
        save_cookie_file: engine.config.save_cookie_file.clone(),
        max_redirects: engine.config.max_redirects,
        connect_timeout: engine.config.connect_timeout,
        netrc_path: engine.config.netrc_path.clone(),
        no_netrc: engine.config.no_netrc,
    };
    let ftp_cfg = raria_ftp::backend::FtpBackendConfig {
        all_proxy: engine.config.all_proxy.clone(),
        no_proxy: engine.config.no_proxy.clone(),
        check_certificate: engine.config.check_certificate,
        ca_certificate: engine.config.ca_certificate.clone(),
    };
    let sftp_cfg = raria_sftp::backend::SftpBackendConfig {
        strict_host_key_check: engine.config.sftp_strict_host_key_check,
        known_hosts_path: engine.config.sftp_known_hosts.clone(),
        private_key_path: engine.config.sftp_private_key.clone(),
        private_key_passphrase: engine.config.sftp_private_key_passphrase.clone(),
        all_proxy: engine.config.all_proxy.clone(),
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
/// did not explicitly set one via `--out`.
fn resolve_output_path(
    engine: &Engine,
    gid: Gid,
    job: &raria_core::job::Job,
    probe: &raria_range::backend::FileProbe,
) -> std::path::PathBuf {
    if job.options.out.is_none() {
        if let Some(filename) = probe.suggested_filename.clone() {
            let path = job
                .out_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(filename);
            engine.registry.update(gid, |job| {
                job.out_path = path.clone();
            });
            return path;
        }
    }
    job.out_path.clone()
}

/// Callback invoked after each segment completes a checkpoint.
type CheckpointFn = Arc<dyn Fn(u32, u64) + Send + Sync>;

#[derive(Debug, Clone)]
struct RangeExecutionContext {
    task_id: TaskId,
    runtime_gid: Gid,
}

/// Plan download segments and restore checkpoint progress from persistent store.
///
/// Returns `(connections, segments, checkpoint_callback)`.
fn plan_download_segments(
    engine: &Engine,
    gid: Gid,
    job: &raria_core::job::Job,
    probe: &raria_range::backend::FileProbe,
) -> (
    u32,
    Vec<raria_core::segment::SegmentState>,
    Option<CheckpointFn>,
) {
    let file_size = probe.size.unwrap_or(0);
    let max_conn = job.options.max_connections;
    let mut resolved_connections = if probe.supports_range && file_size > 0 {
        max_conn.min((file_size / 1024).max(1) as u32)
    } else {
        1
    };
    if probe.supports_range && file_size > 0 && engine.config.min_split_size > 0 {
        let max_by_min = (file_size / engine.config.min_split_size).max(1) as u32;
        resolved_connections = resolved_connections.min(max_by_min);
    }

    engine.registry.update(gid, |job| {
        job.total_size = Some(file_size);
        job.connections = resolved_connections;
    });

    let ranges = if file_size > 0 {
        plan_segments(file_size, resolved_connections)
    } else {
        vec![(0u64, u64::MAX)]
    };
    let mut resolved_segments = init_segment_states(&ranges);

    // Restore checkpoint progress from persistent store.
    if let Some(store) = engine.store() {
        let persisted_result =
            store
                .list_native_segments(&job.task_id)
                .and_then(|native_segments| {
                    if native_segments.is_empty() {
                        store.list_segments(gid)
                    } else {
                        Ok(native_segments)
                    }
                });
        match persisted_result {
            Ok(persisted) if !persisted.is_empty() => {
                for (seg_id, persisted_state) in &persisted {
                    if let Some(seg) = resolved_segments.get_mut(*seg_id as usize) {
                        if persisted_state.downloaded > 0
                            && persisted_state.downloaded <= seg.size()
                        {
                            seg.downloaded = persisted_state.downloaded;
                            seg.status = SegmentStatus::Pending;
                            info!(
                                %gid, seg_id, resumed = persisted_state.downloaded,
                                "resumed segment from checkpoint"
                            );
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                warn!(%gid, error = %e, "failed to load persisted segments, starting fresh");
            }
        }
    }

    let on_checkpoint: Option<CheckpointFn> = engine.store().map(|store| {
        let store = Arc::clone(store);
        let task_id = job.task_id.clone();
        let seg_ranges: Vec<(u64, u64)> =
            resolved_segments.iter().map(|s| (s.start, s.end)).collect();
        Arc::new(move |seg_id: u32, bytes_downloaded: u64| {
            let (start, end) = seg_ranges.get(seg_id as usize).copied().unwrap_or((0, 0));
            let seg = raria_core::segment::SegmentState {
                start,
                end,
                downloaded: bytes_downloaded,
                etag: None,
                status: raria_core::segment::SegmentStatus::Active,
            };
            if let Err(e) = store.put_native_segment(&task_id, seg_id, &seg) {
                tracing::warn!(%gid, task_id = %task_id, seg_id, error = %e, "failed to checkpoint native segment progress");
            }
        }) as CheckpointFn
    });

    (resolved_connections, resolved_segments, on_checkpoint)
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
async fn finalize_complete(engine: &Engine, gid: Gid, downloaded: u64) -> Result<()> {
    let task_id = engine
        .task_id_for_gid(gid)
        .context("native task id not found during completion")?;
    engine.complete_native_task(&task_id, downloaded)?;
    cleanup_segment_checkpoints(engine, gid);
    info!(%gid, bytes = downloaded, "daemon: download complete");
    Ok(())
}

fn reset_for_next_mirror(
    engine: &Engine,
    gid: Gid,
    out_path: &std::path::Path,
    segments: Option<&mut Vec<raria_core::segment::SegmentState>>,
) {
    cleanup_segment_checkpoints(engine, gid);
    engine.registry.update(gid, |job| {
        job.downloaded = 0;
        job.connections = 0;
    });
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

fn next_unattempted_uri(
    uris: &[String],
    attempted_counts: &HashMap<String, usize>,
) -> Option<String> {
    let mut seen_counts: HashMap<&str, usize> = HashMap::new();
    for uri in uris {
        let seen = seen_counts.entry(uri.as_str()).or_insert(0);
        *seen += 1;
        let attempted = attempted_counts.get(uri).copied().unwrap_or(0);
        if *seen > attempted {
            return Some(uri.clone());
        }
    }
    None
}

fn has_unattempted_registered_uri(
    engine: &Engine,
    gid: Gid,
    attempted_counts: &HashMap<String, usize>,
) -> Result<bool> {
    let uris = engine
        .registry
        .get(gid)
        .context("job not found in registry during mirror retry check")?
        .uris;
    Ok(next_unattempted_uri(&uris, attempted_counts).is_some())
}

/// Persist interrupted segment state for future resumption.
fn persist_interrupted_segments(
    engine: &Engine,
    gid: Gid,
    segments: &[raria_core::segment::SegmentState],
    downloaded: u64,
) {
    engine.registry.update(gid, |job| {
        job.downloaded = downloaded;
        job.connections = 0;
    });
    if let Some(store) = engine.store() {
        let task_id = engine.registry.get(gid).map(|job| job.task_id);
        for (seg_id, seg) in segments.iter().enumerate() {
            if let Some(task_id) = &task_id {
                if let Err(e) = store.put_native_segment(task_id, seg_id as u32, seg) {
                    tracing::warn!(%gid, task_id = %task_id, seg_id, error = %e, "failed to persist interrupted native segment state");
                }
            }
        }
    }
    info!(%gid, downloaded, "daemon: download interrupted");
}

async fn run_job_download(
    engine: Arc<Engine>,
    context: RangeExecutionContext,
    cancel: CancellationToken,
    default_headers: Vec<(String, String)>,
) -> Result<()> {
    let gid = context.runtime_gid;
    let job = engine
        .registry
        .get(gid)
        .context("job not found in registry")?;
    anyhow::ensure!(
        job.task_id == context.task_id,
        "runtime bridge does not match native task id"
    );
    let rate_limiter = Some(engine.job_rate_limiter(gid, job.options.max_download_limit));

    let ctx = build_download_context(&engine, &job, &default_headers);

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
    let mut on_checkpoint: Option<CheckpointFn> = None;
    let mut last_error: Option<String> = None;

    let mut attempted_counts: HashMap<String, usize> = HashMap::new();
    loop {
        let current_uris = engine
            .registry
            .get(gid)
            .context("job not found in registry during mirror loop")?
            .uris;
        let Some(uri_str) = next_unattempted_uri(&current_uris, &attempted_counts) else {
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
            [("gid", gid.to_string()), ("uri", redacted_url.clone())],
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
                if has_unattempted_registered_uri(&engine, gid, &attempted_counts)? {
                    record_source_failure(&engine, gid, &redacted_url, &error.to_string());
                }
                last_error = Some(classified_error_message(&error.to_string()));
                continue;
            }
        };

        let candidate_path = out_path.clone().unwrap_or_else(|| job.out_path.clone());
        let control_file_path =
            std::path::PathBuf::from(format!("{}.aria2", candidate_path.display()));
        let probe_headers = build_conditional_get_probe_headers(
            &engine.config,
            &parsed_url,
            &candidate_path,
            &control_file_path,
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
                if has_unattempted_registered_uri(&engine, gid, &attempted_counts)? {
                    record_source_failure(&engine, gid, &redacted_url, &error.to_string());
                }
                last_error = Some(classified_error_message(&error.to_string()));
                continue;
            }
        };

        if out_path.is_none() {
            out_path = Some(resolve_output_path(&engine, gid, &job, &probe));
        }

        let out_path_ref = out_path.as_ref().expect("out_path initialized");
        if probe.not_modified {
            let existing_len = std::fs::metadata(out_path_ref)
                .map(|meta| meta.len())
                .unwrap_or(0);
            if let Err(error) = verify_download_integrity(
                gid,
                out_path_ref,
                job.piece_checksum.as_ref(),
                job.options.checksum.as_deref(),
            )
            .await
            {
                last_error = Some(classified_error_message(&error.to_string()));
                if has_unattempted_registered_uri(&engine, gid, &attempted_counts)? {
                    warn!(%gid, uri = %redacted_url, error = %error, "cached mirror output failed verification, trying next mirror");
                    record_source_failure(&engine, gid, &redacted_url, &error.to_string());
                    emit_integrity_failure_log(gid, &redacted_url, &error.to_string(), true, true);
                    reset_for_next_mirror(&engine, gid, out_path_ref, None);
                    continue;
                }
                emit_integrity_failure_log(gid, &redacted_url, &error.to_string(), true, false);
                reset_for_next_mirror(&engine, gid, out_path_ref, None);
                break;
            }
            return finalize_complete(&engine, gid, existing_len).await;
        }

        if segments.is_none() {
            let (conns, segs, ckpt) = plan_download_segments(&engine, gid, &job, &probe);
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
                job.piece_checksum.as_ref(),
                job.options.checksum.as_deref(),
            )
            .await
            {
                last_error = Some(classified_error_message(&error.to_string()));
                if has_unattempted_registered_uri(&engine, gid, &attempted_counts)? {
                    warn!(%gid, uri = %redacted_url, error = %error, "mirror payload failed verification, trying next mirror");
                    record_source_failure(&engine, gid, &redacted_url, &error.to_string());
                    emit_integrity_failure_log(gid, &redacted_url, &error.to_string(), false, true);
                    reset_for_next_mirror(&engine, gid, out_path_ref, Some(segments_mut));
                    continue;
                }
                emit_integrity_failure_log(gid, &redacted_url, &error.to_string(), false, false);
                reset_for_next_mirror(&engine, gid, out_path_ref, Some(segments_mut));
                break;
            }
            return finalize_complete(&engine, gid, downloaded_total).await;
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
            if has_unattempted_registered_uri(&engine, gid, &attempted_counts)? {
                warn!(%gid, uri = %redacted_url, "mirror failed, trying next mirror");
                record_source_failure(&engine, gid, &redacted_url, &raw_err_msg);
                raria_core::logging::emit_structured_log(
                    "WARN",
                    "raria::daemon",
                    "mirror failed, trying next mirror",
                    [("gid", gid.to_string()), ("uri", redacted_url.clone())],
                );
                continue;
            }

            let task_id = engine
                .task_id_for_gid(gid)
                .context("native task id not found during failure")?;
            engine.fail_native_task(
                &task_id,
                last_error
                    .as_deref()
                    .unwrap_or("transient error: mirror failed"),
            )?;
            return Ok(());
        }

        persist_interrupted_segments(&engine, gid, segments_mut, downloaded_total);
        return Ok(());
    }

    let task_id = engine
        .task_id_for_gid(gid)
        .context("native task id not found during failure")?;
    engine.fail_native_task(
        &task_id,
        last_error
            .as_deref()
            .unwrap_or("transient error: all mirrors failed"),
    )?;
    Ok(())
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
    fn next_unattempted_uri_uses_fresh_registry_order() {
        let mut attempted = HashMap::new();
        attempted.insert("https://primary.example/file.iso".to_string(), 1usize);

        let next = next_unattempted_uri(
            &[
                "https://fallback.example/file.iso".to_string(),
                "https://primary.example/file.iso".to_string(),
            ],
            &attempted,
        );

        assert_eq!(next.as_deref(), Some("https://fallback.example/file.iso"));
    }

    #[test]
    fn next_unattempted_uri_tracks_duplicate_occurrences() {
        let mut attempted = HashMap::new();
        attempted.insert("https://mirror.example/file.iso".to_string(), 1usize);

        let next = next_unattempted_uri(
            &[
                "https://mirror.example/file.iso".to_string(),
                "https://mirror.example/file.iso".to_string(),
            ],
            &attempted,
        );

        assert_eq!(next.as_deref(), Some("https://mirror.example/file.iso"));
    }

    #[test]
    fn interrupted_segment_persistence_does_not_create_legacy_rows_without_runtime_job() {
        let dir = tempdir().expect("tempdir");
        let store_path = dir.path().join("session.redb");
        let store = Arc::new(Store::open(&store_path).expect("store"));
        let engine = Engine::with_store(GlobalConfig::default(), Arc::clone(&store));
        let missing_gid = Gid::from_raw(0xfeed);
        let segments = vec![raria_core::segment::SegmentState {
            start: 0,
            end: 1024,
            downloaded: 512,
            etag: None,
            status: SegmentStatus::Active,
        }];

        persist_interrupted_segments(&engine, missing_gid, &segments, 512);

        assert!(
            store
                .list_segments(missing_gid)
                .expect("legacy segments")
                .is_empty(),
            "native checkpointing must not create legacy gid segment rows"
        );
    }

    #[test]
    fn legacy_gid_segment_rows_remain_read_fallback_for_resume() {
        let dir = tempdir().expect("tempdir");
        let store_path = dir.path().join("session.redb");
        let store = Arc::new(Store::open(&store_path).expect("store"));
        let engine = Engine::with_store(GlobalConfig::default(), Arc::clone(&store));
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["https://example.test/file.bin".to_string()],
                dir: dir.path().to_path_buf(),
                filename: Some("file.bin".to_string()),
                connections: 2,
            })
            .expect("add uri");
        let job = engine.registry.get(handle.gid).expect("job");
        let legacy_segment = raria_core::segment::SegmentState {
            start: 0,
            end: 2048,
            downloaded: 1024,
            etag: None,
            status: SegmentStatus::Active,
        };
        store
            .put_segment(handle.gid, 0, &legacy_segment)
            .expect("legacy segment");
        let probe = raria_range::backend::FileProbe {
            size: Some(4096),
            supports_range: true,
            etag: None,
            last_modified: None,
            content_type: None,
            suggested_filename: None,
            not_modified: false,
        };

        let (_connections, segments, _checkpoint) =
            plan_download_segments(&engine, handle.gid, &job, &probe);

        assert_eq!(segments[0].downloaded, 1024);
        assert_eq!(segments[0].status, SegmentStatus::Pending);
        assert!(
            store
                .list_native_segments(&job.task_id)
                .expect("native segments")
                .is_empty(),
            "legacy fallback reads must not synthesize native rows"
        );
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
        };
        let handle = engine.add_uri(&spec).expect("add uri");
        let mut rx = engine.event_bus.subscribe();
        let cancel = engine.activate_job(handle.gid).expect("activate job");

        let job = engine.registry.get(handle.gid).expect("job");
        run_job_download(
            Arc::clone(&engine),
            RangeExecutionContext {
                task_id: job.task_id,
                runtime_gid: handle.gid,
            },
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
}
