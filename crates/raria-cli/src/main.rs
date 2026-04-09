// raria-cli: Command-line interface for the raria download utility.
//
// Two modes of operation:
//
// 1. `raria download <URL>` — single-shot download (add → activate → download → exit).
// 2. `raria daemon`        — persistent process that:
//    - Starts the Engine with Store persistence
//    - Starts the JSON-RPC server on port 6800
//    - Runs a scheduler loop that activates waiting jobs
//    - Downloads are submitted via RPC and executed concurrently
//
// Integration checklist (Phase B):
// - B1: Engine ↔ Store persistence
// - B2: CancelToken from engine → executor
// - B3: RateLimiter in executor
// - B4: Checksum verification after download
// - B5: Daemon mode with run loop
// - B6: RPC server startup

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use raria_core::checksum;
use raria_core::config::GlobalConfig;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::job::Gid;
use raria_core::limiter::RateLimiter;
use raria_core::persist::Store;
use raria_core::segment::{init_segment_states, plan_segments, SegmentStatus};
use raria_http::backend::HttpBackend;
use raria_range::backend::{ByteSourceBackend, ProbeContext};
use raria_range::executor::{apply_results, total_downloaded, ExecutorConfig, SegmentExecutor};
use raria_rpc::server::{start_rpc_server, RpcServerConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "raria", version, about = "A high-performance download utility")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Maximum concurrent downloads
    #[arg(long, default_value_t = 5, global = true)]
    max_concurrent: u32,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", global = true)]
    log_level: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Download a file from a URL
    Download {
        /// URL to download
        url: String,

        /// Output directory
        #[arg(short = 'd', long, default_value = ".")]
        dir: PathBuf,

        /// Output filename (default: derived from URL)
        #[arg(short = 'o', long)]
        out: Option<String>,

        /// Number of connections
        #[arg(short = 'x', long, default_value_t = 16)]
        connections: u32,

        /// Maximum download speed (bytes/sec, 0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_download_limit: u64,

        /// Checksum for verification (format: algo=hex, e.g. sha-256=abc...)
        #[arg(long)]
        checksum: Option<String>,
    },

    /// Run as a persistent daemon with RPC server
    Daemon {
        /// Output directory for downloads
        #[arg(short = 'd', long, default_value = ".")]
        dir: PathBuf,

        /// Session file for persistence
        #[arg(long, default_value = "raria.session.redb")]
        session_file: PathBuf,

        /// RPC listen port
        #[arg(long, default_value_t = 6800)]
        rpc_port: u16,

        /// Maximum download speed (bytes/sec, 0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_download_limit: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .init();

    match cli.command {
        Commands::Download {
            url,
            dir,
            out,
            connections,
            max_download_limit,
            checksum,
        } => {
            run_download(
                &url,
                &dir,
                out,
                connections,
                cli.max_concurrent,
                max_download_limit,
                checksum,
            )
            .await?;
        }
        Commands::Daemon {
            dir,
            session_file,
            rpc_port,
            max_download_limit,
        } => {
            run_daemon(
                &dir,
                &session_file,
                rpc_port,
                cli.max_concurrent,
                max_download_limit,
            )
            .await?;
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Single-shot download mode
// ═══════════════════════════════════════════════════════════════════════

/// Execute a single download job end-to-end.
async fn run_download(
    url: &str,
    dir: &std::path::Path,
    filename: Option<String>,
    connections: u32,
    max_concurrent: u32,
    max_download_limit: u64,
    checksum_spec: Option<String>,
) -> Result<()> {
    let config = GlobalConfig {
        max_concurrent_downloads: max_concurrent,
        ..Default::default()
    };
    let engine = Engine::new(config);

    let handle = engine.add_uri(&AddUriSpec {
        uris: vec![url.into()],
        dir: dir.to_path_buf(),
        filename,
        connections,
    })?;

    let gid = handle.gid;
    let cancel = engine.activate_job(gid)?;

    let job = engine
        .registry
        .get(gid)
        .context("job vanished from registry")?;

    info!(
        %gid,
        url,
        out = %job.out_path.display(),
        "starting download"
    );

    // Probe the file.
    let backend = Arc::new(HttpBackend::new()?);
    let probe_ctx = ProbeContext::default();
    let parsed_url: url::Url = url.parse().context("invalid URL")?;

    let probe = backend
        .probe(&parsed_url, &probe_ctx)
        .await
        .context("failed to probe URL")?;

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

    // Wire Ctrl+C to the engine's cancel registry (B2).
    let cancel_registry = engine.cancel_registry.clone();
    let ctrl_c_gid = gid;
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down gracefully...");
        cancel_registry.cancel(ctrl_c_gid);
    });

    // Progress callback.
    let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let downloaded_clone = Arc::clone(&downloaded);
    let total = file_size;

    let on_progress: Arc<dyn Fn(u32, u64) + Send + Sync> =
        Arc::new(move |_seg_id, bytes| {
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

    // Build rate limiter (B3).
    let rate_limiter = if max_download_limit > 0 {
        Some(Arc::new(RateLimiter::new(max_download_limit)))
    } else {
        None
    };

    // Execute download.
    let executor = SegmentExecutor::new(ExecutorConfig {
        max_connections: effective_connections,
        max_retries: 5,
        rate_limiter,
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

    eprintln!();

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

        // Checksum verification (B4).
        if let Some(ref spec) = checksum_spec {
            info!("verifying checksum...");
            match checksum::verify_checksum(&job.out_path, spec).await {
                Ok(()) => {
                    info!("checksum verified successfully");
                    println!("Checksum OK");
                }
                Err(e) => {
                    error!(error = %e, "checksum verification failed");
                    anyhow::bail!("checksum verification failed: {e}");
                }
            }
        }

        info!(
            %gid,
            bytes = downloaded_total,
            path = %job.out_path.display(),
            "download complete"
        );
        println!(
            "Download complete: {} ({})",
            job.out_path.display(),
            format_bytes(downloaded_total)
        );
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
        info!(
            %gid,
            downloaded = downloaded_total,
            "download interrupted — can be resumed"
        );
        println!(
            "Download interrupted: {} downloaded so far",
            format_bytes(downloaded_total)
        );
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Daemon mode — persistent run loop with RPC (B5 + B6)
// ═══════════════════════════════════════════════════════════════════════

/// Run raria as a persistent daemon.
///
/// 1. Opens the Store (redb) for persistence.
/// 2. Creates the Engine with Store.
/// 3. Restores any previously-persisted jobs.
/// 4. Starts the RPC server.
/// 5. Enters the scheduler run loop: poll for activatable jobs, spawn
///    SegmentExecutor tasks, collect results, repeat.
/// 6. Shuts down gracefully on Ctrl+C.
async fn run_daemon(
    dir: &std::path::Path,
    session_file: &std::path::Path,
    rpc_port: u16,
    max_concurrent: u32,
    max_download_limit: u64,
) -> Result<()> {
    let config = GlobalConfig {
        dir: dir.to_path_buf(),
        max_concurrent_downloads: max_concurrent,
        enable_rpc: true,
        rpc_listen_port: rpc_port,
        session_file: session_file.to_path_buf(),
        ..Default::default()
    };

    // Ensure download directory exists.
    std::fs::create_dir_all(dir).context("failed to create download directory")?;

    // Open persistence store (B1).
    let store = Arc::new(Store::open(session_file)?);
    let engine = Arc::new(Engine::with_store(config.clone(), Arc::clone(&store)));

    // Restore previously-persisted jobs (B1).
    let restored = engine.restore().unwrap_or_else(|e| {
        warn!(error = %e, "failed to restore jobs from session");
        0
    });
    if restored > 0 {
        info!(count = restored, "restored jobs from session");
    }

    // Wire Ctrl+C to engine shutdown.
    let shutdown_token = engine.shutdown_token();
    let shutdown_clone = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down daemon...");
        shutdown_clone.cancel();
    });

    // Start RPC server (B6).
    let rpc_cancel = CancellationToken::new();
    let rpc_config = RpcServerConfig {
        listen_addr: std::net::SocketAddr::from(([0, 0, 0, 0], rpc_port)),
    };
    let rpc_addr = start_rpc_server(Arc::clone(&engine), &rpc_config, rpc_cancel.clone()).await?;
    info!(%rpc_addr, "RPC server ready");
    println!("raria daemon running — RPC at http://{rpc_addr}/jsonrpc");

    // Build rate limiter (B3).
    let rate_limiter = if max_download_limit > 0 {
        Some(Arc::new(RateLimiter::new(max_download_limit)))
    } else {
        None
    };

    // ── Scheduler run loop ──────────────────────────────────────────
    let work_notify = engine.work_notify();

    loop {
        // Check for shutdown.
        if shutdown_token.is_cancelled() {
            break;
        }

        // Find jobs that can be activated.
        let to_activate = engine.activatable_jobs();

        for gid in to_activate {
            let token = match engine.activate_job(gid) {
                Ok(t) => t,
                Err(e) => {
                    warn!(%gid, error = %e, "failed to activate job");
                    continue;
                }
            };

            // Spawn a download task for this job.
            let engine_ref = Arc::clone(&engine);
            let limiter_ref = rate_limiter.clone();
            let download_dir = dir.to_path_buf();
            tokio::spawn(async move {
                if let Err(e) = run_job_download(engine_ref, gid, token, limiter_ref, download_dir).await {
                    error!(%gid, error = %e, "job download task failed");
                }
            });
        }

        // Wait for new work or shutdown.
        tokio::select! {
            _ = work_notify.notified() => {}
            _ = shutdown_token.cancelled() => { break; }
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
        }
    }

    // Graceful shutdown.
    info!("daemon shutting down...");
    rpc_cancel.cancel();
    // Give in-flight tasks a moment to finish.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    info!("daemon stopped");
    Ok(())
}

/// Execute a single job within the daemon run loop.
///
/// This is spawned as a tokio task for each activated job.
async fn run_job_download(
    engine: Arc<Engine>,
    gid: Gid,
    cancel: CancellationToken,
    rate_limiter: Option<Arc<RateLimiter>>,
    _download_dir: PathBuf,
) -> Result<()> {
    let job = engine
        .registry
        .get(gid)
        .context("job not found in registry")?;

    let uri_str = job.uris.first().context("job has no URIs")?;
    let parsed_url: url::Url = uri_str.parse().context("invalid URI")?;

    info!(%gid, uri = %parsed_url, "daemon: starting download");

    let backend = Arc::new(HttpBackend::new()?);
    let probe_ctx = ProbeContext::default();

    let probe = backend
        .probe(&parsed_url, &probe_ctx)
        .await
        .with_context(|| format!("failed to probe {parsed_url}"))?;

    let file_size = probe.size.unwrap_or(0);
    let max_conn = 16u32; // TODO: from job options.
    let effective_connections = if probe.supports_range && file_size > 0 {
        max_conn.min((file_size / 1024).max(1) as u32)
    } else {
        1
    };

    engine.registry.update(gid, |job| {
        job.total_size = Some(file_size);
    });

    let ranges = if file_size > 0 {
        plan_segments(file_size, effective_connections)
    } else {
        vec![(0u64, u64::MAX)]
    };
    let segments = init_segment_states(&ranges);

    let engine_ref = Arc::clone(&engine);
    let progress_gid = gid;
    let on_progress: Arc<dyn Fn(u32, u64) + Send + Sync> =
        Arc::new(move |_seg_id, bytes| {
            engine_ref.update_progress(progress_gid, bytes);
        });

    let executor = SegmentExecutor::new(ExecutorConfig {
        max_connections: effective_connections,
        max_retries: 5,
        rate_limiter,
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

    let downloaded_total = total_downloaded(&results);
    let all_done = results.iter().all(|r| r.status == SegmentStatus::Done);
    let failed: Vec<_> = results
        .iter()
        .filter(|r| r.status == SegmentStatus::Failed)
        .collect();

    if all_done {
        engine.registry.update(gid, |job| {
            job.downloaded = downloaded_total;
        });
        engine.complete_job(gid)?;
        info!(%gid, bytes = downloaded_total, "daemon: download complete");
    } else if !failed.is_empty() {
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
        });
        info!(%gid, downloaded = downloaded_total, "daemon: download interrupted");
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Utilities
// ═══════════════════════════════════════════════════════════════════════

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
