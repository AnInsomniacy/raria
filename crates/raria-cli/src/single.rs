use crate::backend_factory::create_backend_with_config;
use crate::util::{format_bytes, parse_header_args};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use raria_core::checksum;
use raria_core::config::GlobalConfig;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::limiter::RateLimiter;
use raria_core::segment::{init_segment_states, plan_segments, SegmentStatus};
use raria_range::backend::{ByteSourceBackend, Credentials, ProbeContext};
use raria_range::executor::{apply_results, total_downloaded, ExecutorConfig, SegmentExecutor};
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info};

pub(crate) async fn run_download(
    url: &str,
    dir: &Path,
    filename: Option<String>,
    connections: u32,
    max_concurrent: u32,
    max_download_limit: u64,
    checksum_spec: Option<String>,
    all_proxy: Option<String>,
    check_certificate: bool,
    user_agent: Option<String>,
    http_user: Option<String>,
    http_passwd: Option<String>,
    max_redirect: Option<usize>,
    netrc_path: Option<std::path::PathBuf>,
    no_netrc: bool,
    header_args: Vec<String>,
    timeout_secs: Option<u64>,
    connect_timeout_secs: Option<u64>,
    conditional_get: bool,
    allow_overwrite: bool,
    sftp_strict_host_key_check: bool,
    sftp_known_hosts: Option<std::path::PathBuf>,
    sftp_private_key: Option<std::path::PathBuf>,
    sftp_private_key_passphrase: Option<String>,
    quiet: bool,
) -> Result<()> {
    let headers = parse_header_args(&header_args)?;
    let config = GlobalConfig {
        max_concurrent_downloads: max_concurrent,
        all_proxy: all_proxy.clone(),
        check_certificate,
        user_agent: user_agent.clone(),
        max_redirects: max_redirect,
        netrc_path,
        no_netrc,
        timeout: timeout_secs,
        connect_timeout: connect_timeout_secs,
        conditional_get,
        allow_overwrite,
        sftp_strict_host_key_check,
        sftp_known_hosts,
        sftp_private_key,
        sftp_private_key_passphrase,
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
        user_agent: config.user_agent.clone(),
        cookie_file: config.cookie_file.clone(),
        max_redirects: config.max_redirects,
        connect_timeout: config.connect_timeout,
        netrc_path: config.netrc_path.clone(),
        no_netrc: config.no_netrc,
    };
    let sftp_cfg = raria_sftp::backend::SftpBackendConfig {
        strict_host_key_check: config.sftp_strict_host_key_check,
        known_hosts_path: config.sftp_known_hosts.clone(),
        private_key_path: config.sftp_private_key.clone(),
        private_key_passphrase: config.sftp_private_key_passphrase.clone(),
    };
    let backend = create_backend_with_config(url, Some(&http_cfg), Some(&sftp_cfg))?;
    let probe_timeout = std::time::Duration::from_secs(config.timeout.unwrap_or(30));
    let parsed_url: url::Url = url.parse().context("invalid URL")?;
    let auth = http_user.map(|username| Credentials {
        username,
        password: http_passwd.unwrap_or_default(),
    });
    let fallback_filename = filename.clone().or_else(|| {
        parsed_url
            .path_segments()
            .and_then(|mut segments| segments.next_back().map(str::to_string))
            .filter(|segment| !segment.is_empty())
    });
    let candidate_path = dir.join(
        fallback_filename
            .clone()
            .unwrap_or_else(|| "download".to_string()),
    );
    let control_file_path = std::path::PathBuf::from(format!("{}.aria2", candidate_path.display()));

    let mut probe_headers = headers.clone();
    if config.conditional_get
        && config.allow_overwrite
        && matches!(parsed_url.scheme(), "http" | "https")
        && candidate_path.is_file()
        && !control_file_path.exists()
    {
        let modified = std::fs::metadata(&candidate_path)
            .and_then(|meta| meta.modified())
            .context("failed to read local file mtime for conditional-get")?;
        let modified: DateTime<Utc> = modified.into();
        probe_headers.push((
            "If-Modified-Since".into(),
            modified.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        ));
    }

    let probe = backend
        .probe(
            &parsed_url,
            &ProbeContext {
                headers: probe_headers,
                auth: auth.clone(),
                timeout: probe_timeout,
                ..ProbeContext::default()
            },
        )
        .await
        .context("failed to probe URL")?;

    if probe.not_modified {
        println!("Not modified: {}", candidate_path.display());
        return Ok(());
    }

    let resolved_filename = filename.clone().or_else(|| probe.suggested_filename.clone()).or_else(|| {
        parsed_url
            .path_segments()
            .and_then(|mut segments| segments.next_back().map(str::to_string))
            .filter(|segment| !segment.is_empty())
    });

    let handle = engine.add_uri(&AddUriSpec {
        uris: vec![url.into()],
        dir: dir.to_path_buf(),
        filename: resolved_filename,
        connections,
    })?;

    let gid = handle.gid;
    let cancel = engine.activate_job(gid)?;
    let job = engine
        .registry
        .get(gid)
        .context("job vanished from registry")?;

    info!(%gid, url, out = %job.out_path.display(), "starting download");

    let file_size = probe.size.unwrap_or(0);
    let effective_connections = if probe.supports_range && file_size > 0 {
        connections.min((file_size / 1024).max(1) as u32)
    } else {
        1
    };

    info!(
        file_size,
        supports_range = probe.supports_range,
        connections = effective_connections,
        "probe complete"
    );

    engine.registry.update(gid, |job| {
        job.total_size = Some(file_size);
    });

    let ranges = if file_size > 0 {
        plan_segments(file_size, effective_connections)
    } else {
        vec![(0u64, u64::MAX)]
    };
    let mut segments = init_segment_states(&ranges);

    let cancel_registry = engine.cancel_registry.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down gracefully...");
        cancel_registry.cancel(gid);
    });

    let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let downloaded_clone = Arc::clone(&downloaded);
    let total = file_size;
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

    let rate_limiter = if max_download_limit > 0 {
        Some(Arc::new(RateLimiter::new(max_download_limit)))
    } else {
        None
    };

    let executor = SegmentExecutor::new(ExecutorConfig {
        max_connections: effective_connections,
        max_retries: 5,
        rate_limiter,
        file_allocation: config.file_allocation,
        request_timeout: std::time::Duration::from_secs(config.timeout.unwrap_or(60)),
        request_headers: headers,
        request_auth: auth,
        ..Default::default()
    });

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

    if !quiet {
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

        if let Some(ref spec) = checksum_spec {
            info!("verifying checksum...");
            match checksum::verify_checksum(&job.out_path, spec).await {
                Ok(()) => {
                    info!("checksum verified successfully");
                    if !quiet {
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
        if !quiet {
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
        if !quiet {
            println!(
                "Download interrupted: {} downloaded so far",
                format_bytes(downloaded_total)
            );
        }
    }

    Ok(())
}
