use crate::backend_factory::create_backend_with_config;
use crate::bt_runtime::run_bt_download;
use crate::util::parse_header_args;
use anyhow::{Context, Result};
use raria_core::config::GlobalConfig;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::job::Gid;
use raria_core::persist::Store;
use raria_core::segment::{init_segment_states, plan_segments, SegmentStatus};
use raria_range::backend::{ByteSourceBackend, Credentials, ProbeContext};
use raria_range::executor::{apply_results, total_downloaded, ExecutorConfig, SegmentExecutor};
use raria_rpc::server::{start_rpc_server, RpcServerConfig};
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

    let rpc_cancel = CancellationToken::new();
    let rpc_config = RpcServerConfig {
        listen_addr: std::net::SocketAddr::from(([0, 0, 0, 0], rpc_port)),
    };
    let rpc_addrs = start_rpc_server(Arc::clone(&engine), &rpc_config, rpc_cancel.clone()).await?;
    info!(rpc = %rpc_addrs.rpc, "RPC server ready");
    if !config.quiet {
        println!("raria daemon running — RPC at http://{}/jsonrpc", rpc_addrs.rpc);
    }

    let rate_limiter = Some(Arc::clone(&engine.global_rate_limiter));

    let work_notify = engine.work_notify();

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
                        ).await {
                            error!(%gid, error = %e, "job download task failed");
                        }
                    });
                }
                raria_core::job::JobKind::Bt => {
                    tokio::spawn(async move {
                        if let Err(e) = run_bt_download(engine_ref, gid, token, download_dir).await {
                            error!(%gid, error = %e, "BT download task failed");
                        }
                    });
                }
            }
        }

        tokio::select! {
            _ = work_notify.notified() => {}
            _ = shutdown_token.cancelled() => { break; }
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
    let mut request_headers = default_headers;
    request_headers.extend(job.options.headers.clone());
    let request_auth = job.options.http_user.as_ref().map(|username| Credentials {
        username: username.clone(),
        password: job.options.http_passwd.clone().unwrap_or_default(),
    }).or_else(|| {
        engine.config.http_user.as_ref().map(|username| Credentials {
            username: username.clone(),
            password: engine.config.http_passwd.clone().unwrap_or_default(),
        })
    });

    let uri_str = job.uris.first().context("job has no URIs")?;
    let parsed_url: url::Url = uri_str.parse().context("invalid URI")?;

    info!(%gid, uri = %parsed_url, "daemon: starting download");

    let http_cfg = raria_http::backend::HttpBackendConfig {
        all_proxy: engine.config.all_proxy.clone(),
        http_proxy: engine.config.http_proxy.clone(),
        https_proxy: engine.config.https_proxy.clone(),
        no_proxy: engine.config.no_proxy.clone(),
        check_certificate: engine.config.check_certificate,
        ca_certificate: engine.config.ca_certificate.clone(),
        user_agent: engine.config.user_agent.clone(),
        cookie_file: engine.config.cookie_file.clone(),
        max_redirects: engine.config.max_redirects,
        connect_timeout: engine.config.connect_timeout,
        netrc_path: engine.config.netrc_path.clone(),
        no_netrc: engine.config.no_netrc,
    };
    let sftp_cfg = raria_sftp::backend::SftpBackendConfig {
        strict_host_key_check: engine.config.sftp_strict_host_key_check,
        known_hosts_path: engine.config.sftp_known_hosts.clone(),
        private_key_path: engine.config.sftp_private_key.clone(),
        private_key_passphrase: engine.config.sftp_private_key_passphrase.clone(),
    };
    let backend = create_backend_with_config(uri_str, Some(&http_cfg), Some(&sftp_cfg))?;
    let probe_ctx = ProbeContext {
        headers: request_headers.clone(),
        auth: request_auth.clone(),
        timeout: std::time::Duration::from_secs(engine.config.timeout.unwrap_or(30)),
    };

    let probe = backend
        .probe(&parsed_url, &probe_ctx)
        .await
        .with_context(|| format!("failed to probe {parsed_url}"))?;

    let out_path = if job.options.out.is_none() {
        if let Some(filename) = probe.suggested_filename.clone() {
            let path = job
                .out_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(filename);
            engine.registry.update(gid, |job| {
                job.out_path = path.clone();
            });
            path
        } else {
            job.out_path.clone()
        }
    } else {
        job.out_path.clone()
    };

    let file_size = probe.size.unwrap_or(0);
    let max_conn = job.options.max_connections;
    let effective_connections = if probe.supports_range && file_size > 0 {
        max_conn.min((file_size / 1024).max(1) as u32)
    } else {
        1
    };

    engine.registry.update(gid, |job| {
        job.total_size = Some(file_size);
        job.connections = effective_connections;
    });

    let ranges = if file_size > 0 {
        plan_segments(file_size, effective_connections)
    } else {
        vec![(0u64, u64::MAX)]
    };
    let mut segments = init_segment_states(&ranges);

    if let Some(store) = engine.store() {
        match store.list_segments(gid) {
            Ok(persisted) if !persisted.is_empty() => {
                for (seg_id, persisted_state) in &persisted {
                    if let Some(seg) = segments.get_mut(*seg_id as usize) {
                        if persisted_state.downloaded > 0 && persisted_state.downloaded <= seg.size() {
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

    let engine_ref = Arc::clone(&engine);
    let on_progress: Arc<dyn Fn(u32, u64) + Send + Sync> = Arc::new(move |_seg_id, bytes| {
        engine_ref.update_progress(gid, bytes);
    });

    let on_checkpoint: Option<Arc<dyn Fn(u32, u64) + Send + Sync>> = engine.store().map(|store| {
        let store = Arc::clone(store);
        let seg_ranges: Vec<(u64, u64)> = segments.iter().map(|s| (s.start, s.end)).collect();
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
        }) as Arc<dyn Fn(u32, u64) + Send + Sync>
    });

    let executor = SegmentExecutor::new(ExecutorConfig {
        max_connections: effective_connections,
        max_retries: 5,
        rate_limiter,
        on_checkpoint,
        file_allocation: engine.config.file_allocation,
        request_timeout: std::time::Duration::from_secs(engine.config.timeout.unwrap_or(60)),
        request_headers,
        request_auth,
        ..Default::default()
    });

    let results = executor
        .execute(
            backend as Arc<dyn ByteSourceBackend>,
            &parsed_url,
            &out_path,
            &segments,
            cancel,
            on_progress,
        )
        .await?;

    let downloaded_total = total_downloaded(&results);
    apply_results(&mut segments, &results);
    let all_done = results.iter().all(|r| r.status == SegmentStatus::Done);
    let failed: Vec<_> = results
        .iter()
        .filter(|r| r.status == SegmentStatus::Failed)
        .collect();

    if all_done {
        engine.registry.update(gid, |job| {
            job.downloaded = downloaded_total;
            job.connections = 0;
        });
        engine.complete_job(gid)?;
        if let Some(store) = engine.store() {
            if let Err(e) = store.remove_segments(gid) {
                tracing::warn!(%gid, error = %e, "failed to clean up segment checkpoints");
            }
        }
        info!(%gid, bytes = downloaded_total, "daemon: download complete");
    } else if !failed.is_empty() {
        engine.registry.update(gid, |job| {
            job.connections = 0;
        });
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
        engine.fail_job(gid, &err_msg)?;
    } else {
        engine.registry.update(gid, |job| {
            job.downloaded = downloaded_total;
            job.connections = 0;
        });
        if let Some(store) = engine.store() {
            for (seg_id, seg) in segments.iter().enumerate() {
                if let Err(e) = store.put_segment(gid, seg_id as u32, seg) {
                    tracing::warn!(%gid, seg_id, error = %e, "failed to persist interrupted segment state");
                }
            }
        }
        info!(%gid, downloaded = downloaded_total, "daemon: download interrupted");
    }

    Ok(())
}
