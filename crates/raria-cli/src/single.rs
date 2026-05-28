use crate::backend_factory::create_backend_with_config;
use crate::daemon::{load_ed2k_bootstrap_state, run_ed2k_download};
use crate::executor_config::apply_global_retry_policy;
use crate::util::{
    build_conditional_get_probe_headers, format_bytes, parse_header_args, redact_url_for_logs,
};
use anyhow::{Context, Result};
use raria_core::checksum;
use raria_core::config::GlobalConfig;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::job::JobKind;
use raria_core::limiter::SharedRateLimiter;
use raria_core::native::{TaskId, TaskLifecycle};
use raria_core::persist::Store;
use raria_core::segment::{SegmentStatus, init_segment_states, plan_segments};
use raria_ed2k::link::{Ed2kLink, parse_link};
use raria_range::backend::{ByteSourceBackend, Credentials, ProbeContext};
use raria_range::executor::{ExecutorConfig, SegmentExecutor, apply_results, total_downloaded};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

pub(crate) struct SingleDownloadOptions {
    pub config: GlobalConfig,
    pub url: String,
    pub dir: PathBuf,
    pub filename: Option<String>,
    pub connections: u32,
    pub resume: bool,
    pub max_concurrent: u32,
    pub max_download_limit: u64,
    pub retry_attempts: Option<u32>,
    pub retry_delay_seconds: Option<u32>,
    pub min_segment_size: Option<u64>,
    pub min_speed: Option<u64>,
    pub max_not_found: Option<u32>,
    pub checksum_spec: Option<String>,
    pub proxy: Option<String>,
    pub check_certificate: Option<bool>,
    pub ca_certificate: Option<PathBuf>,
    pub user_agent: Option<String>,
    pub http_user: Option<String>,
    pub http_password: Option<String>,
    pub save_cookies: Option<PathBuf>,
    pub certificate: Option<PathBuf>,
    pub private_key: Option<PathBuf>,
    pub max_redirect: Option<usize>,
    pub netrc_path: Option<PathBuf>,
    pub no_netrc: bool,
    pub header_args: Vec<String>,
    pub timeout_secs: Option<u64>,
    pub connect_timeout_secs: Option<u64>,
    pub conditional_get: bool,
    pub allow_overwrite: bool,
    pub sftp_strict_host_key_check: bool,
    pub sftp_known_hosts: Option<PathBuf>,
    pub sftp_private_key: Option<PathBuf>,
    pub sftp_private_key_passphrase: Option<String>,
    pub quiet: bool,
}

pub(crate) async fn run_download(options: SingleDownloadOptions) -> Result<()> {
    if is_native_foreground_protocol(&options.url) {
        return run_native_foreground_download(options).await;
    }

    let headers = parse_header_args(&options.header_args)?;
    let config = global_config_for_single_download(&options);
    let engine = Arc::new(Engine::new(config.clone()));
    raria_core::logging::replace_structured_log_context([(
        "session_id",
        engine.session_id.clone(),
    )])?;

    let http_cfg = raria_http::backend::HttpBackendConfig {
        proxy: config.proxy.clone(),
        http_proxy: config.http_proxy.clone(),
        https_proxy: config.https_proxy.clone(),
        no_proxy: config.no_proxy.clone(),
        check_certificate: config.check_certificate,
        ca_certificate: config.ca_certificate.clone(),
        client_certificate: config.certificate.clone(),
        client_private_key: config.private_key.clone(),
        user_agent: config.user_agent.clone(),
        load_cookie_file: config.load_cookie_file.clone(),
        cookie_store_file: config.cookie_store_file.clone(),
        max_redirects: config.max_redirects,
        connect_timeout: config.connect_timeout,
        netrc_path: config.netrc_path.clone(),
        no_netrc: config.no_netrc,
    };
    let ftp_cfg = raria_ftp::backend::FtpBackendConfig {
        proxy: config.proxy.clone(),
        no_proxy: config.no_proxy.clone(),
        check_certificate: config.check_certificate,
        ca_certificate: config.ca_certificate.clone(),
    };
    let sftp_cfg = raria_sftp::backend::SftpBackendConfig {
        strict_host_key_check: config.sftp_strict_host_key_check,
        known_hosts_path: config.sftp_known_hosts.clone(),
        private_key_path: config.sftp_private_key.clone(),
        private_key_passphrase: config.sftp_private_key_passphrase.clone(),
        proxy: config.proxy.clone(),
        no_proxy: config.no_proxy.clone(),
    };
    let backend = create_backend_with_config(
        &options.url,
        Some(&http_cfg),
        Some(&ftp_cfg),
        Some(&sftp_cfg),
    )?;
    let probe_timeout = std::time::Duration::from_secs(config.timeout.unwrap_or(30));
    let parsed_url: url::Url = options.url.parse().context("invalid URL")?;
    let auth = options.http_user.clone().map(|username| Credentials {
        username,
        password: options.http_password.clone().unwrap_or_default(),
    });
    let fallback_filename = options.filename.clone().or_else(|| {
        parsed_url
            .path_segments()
            .and_then(|mut segments| segments.next_back().map(str::to_string))
            .filter(|segment| !segment.is_empty())
    });
    let candidate_path = options.dir.join(
        fallback_filename
            .clone()
            .unwrap_or_else(|| "download".to_string()),
    );
    let probe_headers =
        build_conditional_get_probe_headers(&config, &parsed_url, &candidate_path, &headers)?;

    let probe = backend
        .probe(
            &parsed_url,
            &ProbeContext {
                headers: probe_headers,
                auth: auth.clone(),
                timeout: probe_timeout,
            },
        )
        .await
        .context("failed to probe URL")?;

    if probe.not_modified {
        println!("Not modified: {}", candidate_path.display());
        return Ok(());
    }

    let resolved_filename = options
        .filename
        .clone()
        .or_else(|| probe.suggested_filename.clone())
        .or_else(|| {
            parsed_url
                .path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
                .filter(|segment| !segment.is_empty())
        });

    let handle = engine.add_uri(&AddUriSpec {
        uris: vec![options.url.clone()],
        dir: options.dir.clone(),
        filename: resolved_filename,
        connections: options.connections,
        headers: Vec::new(),
        http_user: None,
        http_password: None,
        checksum: options.checksum_spec.clone(),
    })?;

    let gid = handle.gid;
    let cancel = engine.activate_job(gid)?;
    let job = engine
        .registry
        .get(gid)
        .context("job vanished from registry")?;

    info!(
        %gid,
        url = %redact_url_for_logs(&options.url),
        out = %job.out_path.display(),
        "starting download"
    );

    let file_size = probe.size.unwrap_or(0);
    let mut effective_connections = if probe.supports_range && file_size > 0 {
        options.connections.min((file_size / 1024).max(1) as u32)
    } else {
        1
    };
    if probe.supports_range && file_size > 0 && config.min_segment_size > 0 {
        let max_by_min = (file_size / config.min_segment_size).max(1) as u32;
        effective_connections = effective_connections.min(max_by_min);
    }

    info!(
        file_size,
        supports_range = probe.supports_range,
        connections = effective_connections,
        "probe complete"
    );

    engine.registry.update(gid, |job| {
        job.total_size = Some(file_size);
    });

    let existing_len = if options.resume && probe.supports_range && job.out_path.is_file() {
        std::fs::metadata(&job.out_path)
            .map(|meta| meta.len().min(file_size))
            .unwrap_or(0)
    } else {
        0
    };

    let effective_connections = if existing_len > 0 {
        1
    } else {
        effective_connections
    };

    let ranges = if file_size > 0 {
        plan_segments(file_size, effective_connections)
    } else {
        vec![(0u64, u64::MAX)]
    };
    let mut segments = init_segment_states(&ranges);
    if existing_len > 0 {
        if let Some(first) = segments.first_mut() {
            first.downloaded = existing_len;
        }
        engine.registry.update(gid, |job| {
            job.downloaded = existing_len;
        });
    }

    let engine_for_ctrl_c = Arc::clone(&engine);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down gracefully...");
        engine_for_ctrl_c.shutdown();
    });

    #[cfg(unix)]
    {
        let engine_for_sigterm = Arc::clone(&engine);
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(stream) => stream,
                Err(error) => {
                    error!(error = %error, "failed to install SIGTERM handler");
                    return;
                }
            };

            sigterm.recv().await;
            info!("received SIGTERM, shutting down gracefully...");
            engine_for_sigterm.shutdown();
        });
    }

    let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let downloaded_clone = Arc::clone(&downloaded);
    let total = file_size;
    let quiet = options.quiet;
    let on_progress: Arc<dyn Fn(u32, u64) + Send + Sync> = Arc::new(move |_seg_id, bytes| {
        if quiet {
            return;
        }
        let prev = downloaded_clone.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
        let current = prev + bytes;
        if current / (1024 * 1024) > prev / (1024 * 1024) {
            if total > 0 {
                let pct = (current as f64 / total as f64) * 100.0;
                eprint!(
                    "\r  {:.1}% ({}/{})",
                    pct,
                    format_bytes(current),
                    format_bytes(total)
                );
            } else {
                eprint!("\r  downloaded: {}", format_bytes(current));
            }
        }
    });

    let rate_limiter = if options.max_download_limit > 0 {
        Some(Arc::new(SharedRateLimiter::new(options.max_download_limit)))
    } else {
        None
    };

    let executor_cfg = apply_global_retry_policy(
        ExecutorConfig {
            max_connections: effective_connections,
            rate_limiter,
            file_allocation: config.file_allocation,
            request_timeout: std::time::Duration::from_secs(config.timeout.unwrap_or(60)),
            request_headers: headers,
            request_auth: auth,
            request_etag: probe.etag.clone(),
            ..Default::default()
        },
        &config,
    );
    let executor = SegmentExecutor::new(executor_cfg);

    let results = executor
        .execute(
            backend as Arc<dyn ByteSourceBackend>,
            &parsed_url,
            &job.out_path,
            &segments,
            cancel,
            on_progress,
        )
        .await?;

    apply_results(&mut segments, &results);
    let downloaded_total = total_downloaded(&results);

    if !options.quiet {
        eprintln!();
    }

    let all_done = segments.iter().all(|s| s.status == SegmentStatus::Done);
    let failed: Vec<_> = results
        .iter()
        .filter(|r| r.status == SegmentStatus::Failed)
        .collect();

    if all_done {
        if let Some(ref spec) = options.checksum_spec {
            info!("verifying checksum...");
            match checksum::verify_checksum(&job.out_path, spec).await {
                Ok(()) => {
                    info!("checksum verified successfully");
                    if !options.quiet {
                        println!("Checksum OK");
                    }
                }
                Err(e) => {
                    error!(error = %e, "checksum verification failed");
                    if let Err(remove_error) = std::fs::remove_file(&job.out_path) {
                        if remove_error.kind() != std::io::ErrorKind::NotFound {
                            error!(
                                error = %remove_error,
                                path = %job.out_path.display(),
                                "failed to remove invalid output after checksum failure"
                            );
                        }
                    }
                    anyhow::bail!("checksum verification failed: {e}");
                }
            }
        }

        engine.complete_job(gid)?;
        engine.registry.update(gid, |job| {
            job.downloaded = downloaded_total;
        });

        info!(%gid, bytes = downloaded_total, path = %job.out_path.display(), "download complete");
        if !options.quiet {
            println!(
                "Download complete: {} ({})",
                job.out_path.display(),
                format_bytes(downloaded_total)
            );
        }
    } else if !failed.is_empty() {
        let err_msg = failed
            .iter()
            .map(|r| {
                format!(
                    "segment {}: {}",
                    r.segment_id,
                    r.error.as_deref().unwrap_or("unknown error")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        engine.fail_job(gid, &err_msg)?;
        error!(%gid, err_msg, "download failed");
        anyhow::bail!("download failed: {err_msg}");
    } else {
        engine.registry.update(gid, |job| {
            job.downloaded = downloaded_total;
        });
        info!(%gid, downloaded = downloaded_total, "download interrupted — can be resumed");
        if !options.quiet {
            println!(
                "Download interrupted: {} downloaded so far",
                format_bytes(downloaded_total)
            );
        }
    }

    Ok(())
}

fn is_native_foreground_protocol(uri: &str) -> bool {
    uri.starts_with("ed2k://")
}

async fn run_native_foreground_download(options: SingleDownloadOptions) -> Result<()> {
    let mut config = global_config_for_single_download(&options);
    config.session_file = options.dir.join(".raria-download.session.redb");
    std::fs::create_dir_all(&options.dir).context("failed to create download directory")?;

    let store = Arc::new(Store::open(&config.session_file)?);
    let engine = Arc::new(Engine::with_store(config.clone(), Arc::clone(&store)));
    if options.url.starts_with("ed2k://") {
        load_ed2k_bootstrap_state(&config, store.as_ref())?;
    }

    raria_core::logging::replace_structured_log_context([(
        "session_id",
        engine.session_id.clone(),
    )])?;

    let created = engine.add_native_task(&AddUriSpec {
        uris: vec![options.url.clone()],
        dir: options.dir.clone(),
        filename: native_foreground_filename(&options),
        connections: options.connections,
        headers: parse_header_args(&options.header_args)?,
        http_user: options.http_user.clone(),
        http_password: options.http_password.clone(),
        checksum: options.checksum_spec.clone(),
    })?;
    let activation = engine.activate_native_task(&created.task_id)?;
    let task_id = activation.task_id.clone();
    let foreground_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let signal_token = activation.cancel.clone();
    let signal_seen = Arc::clone(&foreground_cancel);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        signal_seen.store(true, std::sync::atomic::Ordering::SeqCst);
        signal_token.cancel();
    });

    #[cfg(unix)]
    {
        let signal_token = activation.cancel.clone();
        let signal_seen = Arc::clone(&foreground_cancel);
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};

            let Ok(mut sigterm) = signal(SignalKind::terminate()) else {
                return;
            };
            sigterm.recv().await;
            signal_seen.store(true, std::sync::atomic::Ordering::SeqCst);
            signal_token.cancel();
        });
    }

    match activation.kind {
        JobKind::Ed2k => {
            let runtime = tokio::spawn(run_ed2k_download(
                Arc::clone(&engine),
                task_id.clone(),
                activation.cancel.clone(),
            ));
            if !options.quiet {
                println!("Started ED2K task: {}", task_id);
            }
            wait_for_native_foreground_task(
                Arc::clone(&engine),
                task_id.clone(),
                activation.cancel.clone(),
                options.quiet,
            )
            .await?;
            runtime.await.context("ED2K runtime task panicked")??;
        }
        JobKind::Range | JobKind::Bt => unreachable!("only ED2K uses the native foreground path"),
    }

    finish_native_foreground_download(
        &engine,
        &task_id,
        options.quiet,
        foreground_cancel.load(std::sync::atomic::Ordering::SeqCst),
    )
}

async fn wait_for_native_foreground_task(
    engine: Arc<Engine>,
    task_id: TaskId,
    cancel: tokio_util::sync::CancellationToken,
    quiet: bool,
) -> Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = interval.tick() => {
                let summary = engine.native_task_summary(&task_id)?;
                if !quiet {
                    print_native_foreground_status(&summary);
                }
                if matches!(
                    summary.lifecycle,
                    TaskLifecycle::Completed
                        | TaskLifecycle::Failed
                        | TaskLifecycle::Removed
                        | TaskLifecycle::Seeding
                ) {
                    return Ok(());
                }
            }
        }
    }
}

fn print_native_foreground_status(summary: &raria_core::native::NativeTaskSummary) {
    let total = summary
        .total_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "unknown".to_string());
    if let Some(ed2k) = &summary.ed2k {
        println!(
            "{}: {} ({}/{}, sources {}, peers {})",
            summary.task_id,
            summary.lifecycle.as_str(),
            format_bytes(summary.completed_bytes),
            total,
            ed2k.known_sources,
            ed2k.connected_peers
        );
    } else {
        println!(
            "{}: {} ({}/{})",
            summary.task_id,
            summary.lifecycle.as_str(),
            format_bytes(summary.completed_bytes),
            total
        );
    }
}

fn global_config_for_single_download(options: &SingleDownloadOptions) -> GlobalConfig {
    let mut config = options.config.clone();
    config.download_dir = options.dir.clone();
    config.max_concurrent_downloads = options.max_concurrent;
    config.global_download_limit = options.max_download_limit;
    if let Some(proxy) = options.proxy.clone() {
        config.proxy = Some(proxy);
    }
    if let Some(check_certificate) = options.check_certificate {
        config.check_certificate = check_certificate;
    }
    if options.ca_certificate.is_some() {
        config.ca_certificate = options.ca_certificate.clone();
    }
    if options.user_agent.is_some() {
        config.user_agent = options.user_agent.clone();
    }
    if options.max_redirect.is_some() {
        config.max_redirects = options.max_redirect;
    }
    if options.netrc_path.is_some() {
        config.netrc_path = options.netrc_path.clone();
    }
    config.no_netrc = options.no_netrc;
    config.timeout = options.timeout_secs;
    config.connect_timeout = options.connect_timeout_secs;
    config.conditional_get = options.conditional_get;
    config.resume = options.resume;
    if let Some(retry_attempts) = options.retry_attempts {
        config.retry_attempts = retry_attempts;
    }
    if let Some(retry_delay_seconds) = options.retry_delay_seconds {
        config.retry_delay_seconds = retry_delay_seconds;
    }
    if let Some(min_segment_size) = options.min_segment_size {
        config.min_segment_size = min_segment_size;
    }
    if let Some(min_speed) = options.min_speed {
        config.min_speed = min_speed;
    }
    if let Some(max_not_found) = options.max_not_found {
        config.max_not_found = max_not_found;
    }
    config.allow_overwrite = options.allow_overwrite || options.resume;
    config.sftp_strict_host_key_check = options.sftp_strict_host_key_check;
    if options.sftp_known_hosts.is_some() {
        config.sftp_known_hosts = options.sftp_known_hosts.clone();
    }
    if options.sftp_private_key.is_some() {
        config.sftp_private_key = options.sftp_private_key.clone();
    }
    if options.sftp_private_key_passphrase.is_some() {
        config.sftp_private_key_passphrase = options.sftp_private_key_passphrase.clone();
    }
    if options.save_cookies.is_some() {
        config.cookie_store_file = options.save_cookies.clone();
    }
    if options.certificate.is_some() {
        config.certificate = options.certificate.clone();
    }
    if options.private_key.is_some() {
        config.private_key = options.private_key.clone();
    }
    config
}

fn native_foreground_filename(options: &SingleDownloadOptions) -> Option<String> {
    options.filename.clone().or_else(|| {
        if let Ok(Ed2kLink::File(file)) = parse_link(&options.url) {
            Some(file.name)
        } else {
            None
        }
    })
}

fn finish_native_foreground_download(
    engine: &Engine,
    task_id: &TaskId,
    quiet: bool,
    cancelled: bool,
) -> Result<()> {
    let summary = engine.native_task_summary(task_id)?;
    if cancelled {
        if !quiet {
            println!(
                "Download cancelled: {} ({}/{})",
                summary.task_id,
                format_bytes(summary.completed_bytes),
                summary
                    .total_bytes
                    .map(format_bytes)
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
        anyhow::bail!("download cancelled");
    }
    match summary.lifecycle {
        TaskLifecycle::Completed => {
            if !quiet {
                println!(
                    "Download complete: {} ({})",
                    summary.output_path.display(),
                    format_bytes(summary.completed_bytes)
                );
            }
            Ok(())
        }
        TaskLifecycle::Failed => {
            anyhow::bail!(
                "{}",
                summary
                    .error_message
                    .unwrap_or_else(|| "download failed".to_string())
            );
        }
        TaskLifecycle::Running | TaskLifecycle::Queued | TaskLifecycle::Paused => {
            if !quiet {
                println!(
                    "Download still running: {} ({}/{})",
                    summary.task_id,
                    format_bytes(summary.completed_bytes),
                    summary
                        .total_bytes
                        .map(format_bytes)
                        .unwrap_or_else(|| "unknown".to_string())
                );
            }
            Ok(())
        }
        TaskLifecycle::Seeding => {
            if !quiet {
                println!(
                    "Download complete: {} (seeding)",
                    summary.output_path.display()
                );
            }
            Ok(())
        }
        TaskLifecycle::Removed => anyhow::bail!("download was removed"),
    }
}
