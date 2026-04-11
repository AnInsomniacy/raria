use crate::backend_factory::create_backend_with_config;
use crate::bt_runtime::run_bt_download;
use crate::executor_config::apply_global_retry_policy;
use crate::hooks::{HookConfig, spawn_hook_runner};
use crate::util::parse_header_args;
use anyhow::{Context, Result};
use raria_core::config::GlobalConfig;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::job::Gid;
use raria_core::persist::Store;
use raria_core::segment::{SegmentStatus, init_segment_states, plan_segments};
use raria_range::backend::{ByteSourceBackend, Credentials, ProbeContext};
use raria_range::executor::{ExecutorConfig, SegmentExecutor, apply_results};
use raria_rpc::server::{RpcServerConfig, start_rpc_server};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub(crate) async fn run_daemon_with_config(
    config: GlobalConfig,
    session_file: &std::path::Path,
    input_uris: Vec<String>,
    download_dir: PathBuf,
    header_args: Vec<String>,
) -> Result<()> {
    let default_headers = parse_header_args(&header_args)?;
    let rpc_port = config.rpc_listen_port;

    std::fs::create_dir_all(&config.dir).context("failed to create download directory")?;

    let store = Arc::new(Store::open(session_file)?);
    let engine = Arc::new(Engine::with_store(config.clone(), Arc::clone(&store)));

    let restored = engine.restore().unwrap_or_else(|e| {
        warn!(error = %e, "failed to restore jobs from session");
        0
    });
    if restored > 0 {
        info!(count = restored, "restored jobs from session");
    }

    for uri_line in &input_uris {
        let uris: Vec<String> = uri_line.split('\t').map(|s| s.to_string()).collect();
        let spec = AddUriSpec {
            uris,
            filename: None,
            dir: download_dir.clone(),
            connections: 1,
        };
        match engine.add_uri(&spec) {
            Ok(handle) => info!(gid = %handle.gid, "added job from input file"),
            Err(e) => warn!(uri = %uri_line, error = %e, "failed to add URI from input file"),
        }
    }

    let shutdown_token = engine.shutdown_token();
    let shutdown_clone = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down daemon...");
        shutdown_clone.cancel();
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
        let shutdown_ref = shutdown_token.clone();
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
            shutdown_ref.cancel();
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
            "raria daemon running — RPC at http://{}/jsonrpc",
            rpc_addrs.rpc
        );
    }

    let rate_limiter = Some(Arc::clone(&engine.global_rate_limiter));

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

        let to_activate = engine.activatable_jobs();

        for gid in to_activate {
            let token = match engine.activate_job(gid) {
                Ok(t) => t,
                Err(e) => {
                    warn!(%gid, error = %e, "failed to activate job");
                    continue;
                }
            };

            let engine_ref = Arc::clone(&engine);
            let limiter_ref = rate_limiter.clone();
            let download_dir = config.dir.clone();

            let job_kind = engine
                .registry
                .get(gid)
                .map(|j| j.kind)
                .unwrap_or(raria_core::job::JobKind::Range);

            match job_kind {
                raria_core::job::JobKind::Range => {
                    let default_headers = default_headers.clone();
                    tokio::spawn(async move {
                        if let Err(e) = run_job_download(
                            engine_ref,
                            gid,
                            token,
                            limiter_ref,
                            default_headers.clone(),
                        )
                        .await
                        {
                            error!(%gid, error = %e, "job download task failed");
                        }
                    });
                }
                raria_core::job::JobKind::Bt => {
                    tokio::spawn(async move {
                        if let Err(e) = run_bt_download(engine_ref, gid, token, download_dir).await
                        {
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
    for job in engine.registry.by_status(raria_core::job::Status::Active) {
        engine.cancel_registry.cancel(job.gid);
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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

/// Plan download segments and restore checkpoint progress from persistent store.
///
/// Returns `(connections, segments, checkpoint_callback)`.
fn plan_download_segments(
    engine: &Engine,
    gid: Gid,
    job: &raria_core::job::Job,
    probe: &raria_range::backend::FileProbe,
) -> (u32, Vec<raria_core::segment::SegmentState>, Option<CheckpointFn>) {
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
        match store.list_segments(gid) {
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

    let on_checkpoint: Option<CheckpointFn> =
        engine.store().map(|store| {
            let store = Arc::clone(store);
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
                if let Err(e) = store.put_segment(gid, seg_id, &seg) {
                    tracing::warn!(%gid, seg_id, error = %e, "failed to checkpoint segment progress");
                }
            }) as CheckpointFn
        });

    (resolved_connections, resolved_segments, on_checkpoint)
}

/// Finalize a completed download: update registry, clean up checkpoints, log.
fn finalize_complete(engine: &Engine, gid: Gid, downloaded: u64) -> Result<()> {
    engine.registry.update(gid, |job| {
        job.downloaded = downloaded;
        job.connections = 0;
    });
    engine.complete_job(gid)?;
    if let Some(store) = engine.store() {
        if let Err(e) = store.remove_segments(gid) {
            tracing::warn!(%gid, error = %e, "failed to clean up segment checkpoints");
        }
    }
    info!(%gid, bytes = downloaded, "daemon: download complete");
    Ok(())
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
        for (seg_id, seg) in segments.iter().enumerate() {
            if let Err(e) = store.put_segment(gid, seg_id as u32, seg) {
                tracing::warn!(%gid, seg_id, error = %e, "failed to persist interrupted segment state");
            }
        }
    }
    info!(%gid, downloaded, "daemon: download interrupted");
}

async fn run_job_download(
    engine: Arc<Engine>,
    gid: Gid,
    cancel: CancellationToken,
    rate_limiter: Option<Arc<raria_core::limiter::SharedRateLimiter>>,
    default_headers: Vec<(String, String)>,
) -> Result<()> {
    let job = engine
        .registry
        .get(gid)
        .context("job not found in registry")?;

    let ctx = build_download_context(&engine, &job, &default_headers);

    let engine_ref = Arc::clone(&engine);
    let on_progress: Arc<dyn Fn(u32, u64) + Send + Sync> = Arc::new(move |_seg_id, bytes| {
        engine_ref.update_progress(gid, bytes);
    });

    let mut out_path: Option<std::path::PathBuf> = None;
    let mut effective_connections: Option<u32> = None;
    let mut segments: Option<Vec<raria_core::segment::SegmentState>> = None;
    let mut on_checkpoint: Option<CheckpointFn> = None;
    let mut last_error: Option<String> = None;

    for (uri_index, uri_str) in job.uris.iter().enumerate() {
        let parsed_url: url::Url = uri_str.parse().context("invalid URI")?;
        info!(%gid, uri = %parsed_url, "daemon: starting download");

        let backend = match create_backend_with_config(
            uri_str,
            Some(&ctx.http_cfg),
            Some(&ctx.ftp_cfg),
            Some(&ctx.sftp_cfg),
        ) {
            Ok(backend) => backend,
            Err(error) => {
                warn!(%gid, uri = %parsed_url, error = %error, "failed to create backend for mirror");
                last_error = Some(error.to_string());
                continue;
            }
        };

        let probe = match backend.probe(&parsed_url, &ctx.probe_ctx).await {
            Ok(probe) => probe,
            Err(error) => {
                warn!(%gid, uri = %parsed_url, error = %error, "failed to probe mirror");
                last_error = Some(error.to_string());
                continue;
            }
        };

        if out_path.is_none() {
            out_path = Some(resolve_output_path(&engine, gid, &job, &probe));
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
            return finalize_complete(&engine, gid, downloaded_total);
        }

        if !failed.is_empty() {
            let err_msg = failed
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
            last_error = Some(err_msg);
            if uri_index + 1 < job.uris.len() {
                warn!(%gid, uri = %parsed_url, "mirror failed, trying next mirror");
                continue;
            }

            engine.registry.update(gid, |job| {
                job.connections = 0;
            });
            engine.fail_job(gid, last_error.as_deref().unwrap_or("mirror failed"))?;
            return Ok(());
        }

        persist_interrupted_segments(&engine, gid, segments_mut, downloaded_total);
        return Ok(());
    }

    engine.registry.update(gid, |job| {
        job.connections = 0;
    });
    engine.fail_job(gid, last_error.as_deref().unwrap_or("all mirrors failed"))?;
    Ok(())
}
