use crate::backend_factory::create_backend_with_config;
use crate::executor_config::apply_global_retry_policy;
use crate::util::{build_conditional_get_probe_headers, format_bytes, parse_header_args};
use anyhow::{Context, Result};
use raria_core::checksum;
use raria_core::config::GlobalConfig;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::limiter::SharedRateLimiter;
use raria_core::segment::{SegmentStatus, init_segment_states, plan_segments};
use raria_range::backend::{ByteSourceBackend, Credentials, ProbeContext};
use raria_range::executor::{ExecutorConfig, SegmentExecutor, apply_results, total_downloaded};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

pub(crate) struct SingleDownloadOptions {
    pub url: String,
    pub dir: PathBuf,
    pub filename: Option<String>,
    pub connections: u32,
    pub continue_download: bool,
    pub max_concurrent: u32,
    pub max_download_limit: u64,
    pub max_tries: Option<u32>,
    pub retry_wait: Option<u32>,
    pub min_split_size: Option<u64>,
    pub lowest_speed_limit: Option<u64>,
    pub max_file_not_found: Option<u32>,
    pub checksum_spec: Option<String>,
    pub all_proxy: Option<String>,
    pub check_certificate: bool,
    pub ca_certificate: Option<PathBuf>,
    pub user_agent: Option<String>,
    pub http_user: Option<String>,
    pub http_passwd: Option<String>,
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
    let headers = parse_header_args(&options.header_args)?;
    let config = GlobalConfig {
        max_concurrent_downloads: options.max_concurrent,
        all_proxy: options.all_proxy.clone(),
        check_certificate: options.check_certificate,
        ca_certificate: options.ca_certificate.clone(),
        user_agent: options.user_agent.clone(),
        max_redirects: options.max_redirect,
        netrc_path: options.netrc_path.clone(),
        no_netrc: options.no_netrc,
        timeout: options.timeout_secs,
        connect_timeout: options.connect_timeout_secs,
        conditional_get: options.conditional_get,
        continue_download: options.continue_download,
        max_tries: options.max_tries.unwrap_or(5),
        retry_wait: options.retry_wait.unwrap_or(0),
        min_split_size: options.min_split_size.unwrap_or(0),
        lowest_speed_limit: options.lowest_speed_limit.unwrap_or(0),
        max_file_not_found: options.max_file_not_found.unwrap_or(0),
        allow_overwrite: options.allow_overwrite || options.continue_download,
        sftp_strict_host_key_check: options.sftp_strict_host_key_check,
        sftp_known_hosts: options.sftp_known_hosts.clone(),
        sftp_private_key: options.sftp_private_key.clone(),
        sftp_private_key_passphrase: options.sftp_private_key_passphrase.clone(),
        save_cookie_file: options.save_cookies.clone(),
        certificate: options.certificate.clone(),
        private_key: options.private_key.clone(),
        ..Default::default()
    };
    let engine = Engine::new(config.clone());

    let http_cfg = raria_http::backend::HttpBackendConfig {
        all_proxy: config.all_proxy.clone(),
        http_proxy: config.http_proxy.clone(),
        https_proxy: config.https_proxy.clone(),
        no_proxy: config.no_proxy.clone(),
        check_certificate: config.check_certificate,
        ca_certificate: config.ca_certificate.clone(),
        client_certificate: config.certificate.clone(),
        client_private_key: config.private_key.clone(),
        user_agent: config.user_agent.clone(),
        cookie_file: config.cookie_file.clone(),
        save_cookie_file: config.save_cookie_file.clone(),
        max_redirects: config.max_redirects,
        connect_timeout: config.connect_timeout,
        netrc_path: config.netrc_path.clone(),
        no_netrc: config.no_netrc,
    };
    let ftp_cfg = raria_ftp::backend::FtpBackendConfig {
        all_proxy: config.all_proxy.clone(),
        no_proxy: config.no_proxy.clone(),
        check_certificate: config.check_certificate,
        ca_certificate: config.ca_certificate.clone(),
    };
    let sftp_cfg = raria_sftp::backend::SftpBackendConfig {
        strict_host_key_check: config.sftp_strict_host_key_check,
        known_hosts_path: config.sftp_known_hosts.clone(),
        private_key_path: config.sftp_private_key.clone(),
        private_key_passphrase: config.sftp_private_key_passphrase.clone(),
        all_proxy: config.all_proxy.clone(),
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
        password: options.http_passwd.clone().unwrap_or_default(),
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
    let control_file_path = std::path::PathBuf::from(format!("{}.aria2", candidate_path.display()));

    let probe_headers = build_conditional_get_probe_headers(
        &config,
        &parsed_url,
        &candidate_path,
        &control_file_path,
        &headers,
    )?;

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
    })?;

    let gid = handle.gid;
    let cancel = engine.activate_job(gid)?;
    let job = engine
        .registry
        .get(gid)
        .context("job vanished from registry")?;

    info!(%gid, url = %options.url, out = %job.out_path.display(), "starting download");

    let file_size = probe.size.unwrap_or(0);
    let mut effective_connections = if probe.supports_range && file_size > 0 {
        options.connections.min((file_size / 1024).max(1) as u32)
    } else {
        1
    };
    if probe.supports_range && file_size > 0 && config.min_split_size > 0 {
        let max_by_min = (file_size / config.min_split_size).max(1) as u32;
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

    let existing_len = if options.continue_download
        && probe.supports_range
        && !control_file_path.exists()
        && job.out_path.is_file()
    {
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

    let cancel_registry = engine.cancel_registry.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down gracefully...");
        cancel_registry.cancel(gid);
    });

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
        engine.complete_job(gid)?;
        engine.registry.update(gid, |job| {
            job.downloaded = downloaded_total;
        });

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
                    anyhow::bail!("checksum verification failed: {e}");
                }
            }
        }

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
