mod backend_factory;
mod bt_runtime;
mod daemon;
mod executor_config;
mod hooks;
mod single;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use raria_core::config::GlobalConfig;
use std::ffi::OsString;
use std::path::PathBuf;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[cfg(unix)]
fn spawn_background_daemon(raw_args: &[OsString]) -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let filtered_args: Vec<OsString> = raw_args
        .iter()
        .skip(1)
        .filter(|arg| *arg != "--daemon" && *arg != "-D")
        .cloned()
        .collect();

    std::process::Command::new(current_exe)
        .args(filtered_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn background daemon: {e}"))?;
    Ok(())
}

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

    /// Write structured logs to the given file path.
    #[arg(long, global = true)]
    log: Option<PathBuf>,

    /// Suppress normal user-facing output
    #[arg(long, short = 'q', default_value_t = false, global = true)]
    quiet: bool,

    /// Path to configuration file (aria2-compatible format)
    #[arg(long, global = true)]
    conf_path: Option<PathBuf>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
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

        /// Continue downloading a partially downloaded file.
        #[arg(short = 'c', long = "continue", default_value_t = false)]
        continue_download: bool,

        /// Maximum download speed (bytes/sec, 0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_download_limit: u64,

        /// Maximum retries per segment (aria2: --max-tries, 0 = infinite).
        #[arg(long)]
        max_tries: Option<u32>,

        /// Seconds to wait between retries (aria2: --retry-wait).
        #[arg(long)]
        retry_wait: Option<u32>,

        /// Minimum size in bytes for a split segment (aria2: --min-split-size).
        #[arg(long = "min-split-size")]
        min_split_size: Option<u64>,

        /// Abort connections when download speed is below this limit (bytes/sec).
        #[arg(long = "lowest-speed-limit")]
        lowest_speed_limit: Option<u64>,

        /// Maximum number of file-not-found errors before giving up.
        #[arg(long = "max-file-not-found")]
        max_file_not_found: Option<u32>,

        /// Path to Netscape cookie file for persistence.
        #[arg(long)]
        save_cookies: Option<PathBuf>,

        /// Checksum for verification (format: algo=hex, e.g. sha-256=abc...)
        #[arg(long)]
        checksum: Option<String>,

        /// Proxy URL for all protocols
        #[arg(long)]
        all_proxy: Option<String>,

        /// Disable TLS certificate verification
        #[arg(long)]
        check_certificate: Option<bool>,

        /// Path to custom CA certificate
        #[arg(long)]
        ca_certificate: Option<PathBuf>,

        /// Custom user-agent string
        #[arg(long)]
        user_agent: Option<String>,

        /// Path to client certificate chain for mTLS.
        #[arg(long)]
        certificate: Option<PathBuf>,

        /// Path to client private key for mTLS.
        #[arg(long = "private-key")]
        private_key: Option<PathBuf>,

        /// HTTP Basic auth username
        #[arg(long)]
        http_user: Option<String>,

        /// HTTP Basic auth password
        #[arg(long)]
        http_passwd: Option<String>,

        /// Maximum number of redirects to follow (0 disables redirects)
        #[arg(long)]
        max_redirect: Option<usize>,

        /// Path to a netrc file for host credential lookup
        #[arg(long)]
        netrc_path: Option<PathBuf>,

        /// Disable all netrc credential loading
        #[arg(long, default_value_t = false)]
        no_netrc: bool,

        /// Custom request header. May be specified multiple times.
        #[arg(long)]
        header: Vec<String>,

        /// Request timeout in seconds.
        #[arg(long)]
        timeout: Option<u64>,

        /// Connection establishment timeout in seconds.
        #[arg(long)]
        connect_timeout: Option<u64>,

        /// Only download when the remote resource is newer than the local file.
        #[arg(long, default_value_t = false)]
        conditional_get: bool,

        /// Allow overwriting an existing output file.
        #[arg(long, default_value_t = false)]
        allow_overwrite: bool,

        /// Enable strict SFTP host key verification.
        #[arg(long, default_value_t = false)]
        sftp_strict_host_key_check: bool,

        /// Path to a known_hosts file for SFTP host verification.
        #[arg(long)]
        sftp_known_hosts: Option<PathBuf>,

        /// Path to an SSH private key used for SFTP authentication.
        #[arg(long)]
        sftp_private_key: Option<PathBuf>,

        /// Passphrase for the SSH private key used for SFTP authentication.
        #[arg(long)]
        sftp_private_key_passphrase: Option<String>,
    },

    /// Run as a persistent daemon with RPC server
    Daemon {
        /// Output directory for downloads
        #[arg(short = 'd', long, default_value = ".")]
        dir: PathBuf,

        /// Session file for persistence
        #[arg(long, default_value = "raria.session.redb")]
        session_file: PathBuf,

        /// Detach and keep the daemon running in the background.
        #[arg(short = 'D', long = "daemon", default_value_t = false)]
        daemonize: bool,

        /// Save the current session periodically while running.
        #[arg(long)]
        save_session_interval: Option<u64>,

        /// RPC listen port
        #[arg(long, default_value_t = 6800)]
        rpc_port: u16,

        /// Maximum download speed (bytes/sec, 0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_download_limit: u64,

        /// Maximum retries per segment (aria2: --max-tries, 0 = infinite).
        #[arg(long)]
        max_tries: Option<u32>,

        /// Seconds to wait between retries (aria2: --retry-wait).
        #[arg(long)]
        retry_wait: Option<u32>,

        /// Minimum size in bytes for a split segment (aria2: --min-split-size).
        #[arg(long = "min-split-size")]
        min_split_size: Option<u64>,

        /// Abort connections when download speed is below this limit (bytes/sec).
        #[arg(long = "lowest-speed-limit")]
        lowest_speed_limit: Option<u64>,

        /// Maximum number of file-not-found errors before giving up.
        #[arg(long = "max-file-not-found")]
        max_file_not_found: Option<u32>,

        /// Proxy URL for all protocols
        #[arg(long)]
        all_proxy: Option<String>,

        /// Proxy URL for HTTP only
        #[arg(long)]
        http_proxy: Option<String>,

        /// Proxy URL for HTTPS only
        #[arg(long)]
        https_proxy: Option<String>,

        /// Comma-separated list of no-proxy domains
        #[arg(long)]
        no_proxy: Option<String>,

        /// Disable TLS certificate verification
        #[arg(long, default_value_t = true)]
        check_certificate: bool,

        /// Path to custom CA certificate
        #[arg(long)]
        ca_certificate: Option<PathBuf>,

        /// Optional BT DHT persistence/config file path for librqbit.
        #[arg(long = "bt-dht-config-file")]
        bt_dht_config_file: Option<PathBuf>,

        /// BT piece selection strategy: `current` or `rarest-first`.
        #[arg(long = "bt-piece-strategy")]
        bt_piece_strategy: Option<String>,

        /// Path to client certificate chain for mTLS.
        #[arg(long)]
        certificate: Option<PathBuf>,

        /// Path to client private key for mTLS.
        #[arg(long = "private-key")]
        private_key: Option<PathBuf>,

        /// Custom user-agent string
        #[arg(long)]
        user_agent: Option<String>,

        /// HTTP Basic auth username
        #[arg(long)]
        http_user: Option<String>,

        /// HTTP Basic auth password
        #[arg(long)]
        http_passwd: Option<String>,

        /// Input file containing URIs to download (one per line)
        #[arg(short = 'i', long)]
        input_file: Option<PathBuf>,

        /// Hook script fired when a download starts.
        #[arg(long)]
        on_download_start: Option<PathBuf>,

        /// Hook script fired when a download completes.
        #[arg(long)]
        on_download_complete: Option<PathBuf>,

        /// Hook script fired when a download errors.
        #[arg(long)]
        on_download_error: Option<PathBuf>,

        /// RPC secret token for authentication
        #[arg(long)]
        rpc_secret: Option<String>,

        /// Allow browser clients from any origin to access HTTP JSON-RPC.
        #[arg(long, default_value_t = false)]
        rpc_allow_origin_all: bool,

        /// Path to Netscape cookie file
        #[arg(long)]
        load_cookies: Option<PathBuf>,

        /// Path to Netscape cookie file for persistence
        #[arg(long)]
        save_cookies: Option<PathBuf>,

        /// File allocation strategy: none, prealloc, trunc, falloc
        #[arg(long, default_value = "none")]
        file_allocation: String,

        /// Maximum number of redirects to follow (0 disables redirects)
        #[arg(long)]
        max_redirect: Option<usize>,

        /// Path to a netrc file for host credential lookup
        #[arg(long)]
        netrc_path: Option<PathBuf>,

        /// Disable all netrc credential loading
        #[arg(long, default_value_t = false)]
        no_netrc: bool,

        /// Custom request header. May be specified multiple times.
        #[arg(long)]
        header: Vec<String>,

        /// Request timeout in seconds.
        #[arg(long)]
        timeout: Option<u64>,

        /// Connection establishment timeout in seconds.
        #[arg(long)]
        connect_timeout: Option<u64>,

        /// Only download when the remote resource is newer than the local file.
        #[arg(long, default_value_t = false)]
        conditional_get: bool,

        /// Allow overwriting an existing output file.
        #[arg(long, default_value_t = false)]
        allow_overwrite: bool,

        /// Enable strict SFTP host key verification.
        #[arg(long, default_value_t = false)]
        sftp_strict_host_key_check: bool,

        /// Path to a known_hosts file for SFTP host verification.
        #[arg(long)]
        sftp_known_hosts: Option<PathBuf>,

        /// Path to an SSH private key used for SFTP authentication.
        #[arg(long)]
        sftp_private_key: Option<PathBuf>,

        /// Passphrase for the SSH private key used for SFTP authentication.
        #[arg(long)]
        sftp_private_key_passphrase: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let cli = Cli::parse();

    #[cfg(unix)]
    {
        let daemonize_requested = matches!(
            &cli.command,
            Commands::Daemon {
                daemonize: true,
                ..
            }
        );
        if daemonize_requested {
            spawn_background_daemon(&raw_args)?;
            // Exit immediately to ensure the parent returns promptly even under load.
            // The detached child continues running the daemon process.
            std::process::exit(0);
        }
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level));
    let _log_guard: Option<tracing_appender::non_blocking::WorkerGuard> =
        if let Some(ref log_path) = cli.log {
            let directory = log_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            std::fs::create_dir_all(&directory)?;
            raria_core::logging::install_structured_log_file(log_path)?;
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(std::io::sink)
                .init();
            None
        } else if cli.quiet {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(std::io::sink)
                .init();
            None
        } else {
            tracing_subscriber::fmt().with_env_filter(env_filter).init();
            None
        };

    info!(
        component = "logging",
        event = "initialized",
        "logging initialized"
    );
    raria_core::logging::emit_structured_log(
        "INFO",
        "raria::logging",
        "logging initialized",
        [
            ("component", "logging".to_string()),
            ("event", "initialized".to_string()),
        ],
    );

    let mut base_config = GlobalConfig::default();
    if let Some(ref conf_path) = cli.conf_path {
        use raria_core::config_file::{ConfigParseMode, load_config_file_with_mode};
        // Daemon mode: fail-fast on invalid config values.
        // Single download: tolerate invalid values for aria2 compatibility.
        let mode = if matches!(&cli.command, Commands::Daemon { .. }) {
            ConfigParseMode::Strict
        } else {
            ConfigParseMode::Lenient
        };
        match load_config_file_with_mode(&mut base_config, conf_path, mode) {
            Ok(()) => info!(path = %conf_path.display(), "loaded configuration file"),
            Err(e) if mode == ConfigParseMode::Strict => {
                error!(
                    path = %conf_path.display(), error = %e,
                    "invalid configuration — daemon mode requires valid config"
                );
                std::process::exit(1);
            }
            Err(e) => warn!(
                path = %conf_path.display(), error = %e,
                "failed to load configuration file"
            ),
        }
    }

    match cli.command {
        Commands::Download {
            url,
            dir,
            out,
            connections,
            continue_download,
            max_download_limit,
            max_tries,
            retry_wait,
            min_split_size,
            lowest_speed_limit,
            max_file_not_found,
            save_cookies,
            checksum,
            all_proxy,
            check_certificate,
            ca_certificate,
            certificate,
            private_key,
            user_agent,
            http_user,
            http_passwd,
            max_redirect,
            netrc_path,
            no_netrc,
            header,
            timeout,
            connect_timeout,
            conditional_get,
            allow_overwrite,
            sftp_strict_host_key_check,
            sftp_known_hosts,
            sftp_private_key,
            sftp_private_key_passphrase,
        } => {
            single::run_download(single::SingleDownloadOptions {
                url,
                dir,
                filename: out,
                connections,
                continue_download,
                max_concurrent: cli.max_concurrent,
                max_download_limit,
                max_tries,
                retry_wait,
                min_split_size,
                lowest_speed_limit,
                max_file_not_found,
                save_cookies,
                checksum_spec: checksum,
                all_proxy,
                check_certificate: check_certificate.unwrap_or(true),
                ca_certificate,
                certificate,
                private_key,
                user_agent,
                http_user,
                http_passwd,
                max_redirect,
                netrc_path,
                no_netrc,
                header_args: header,
                timeout_secs: timeout,
                connect_timeout_secs: connect_timeout,
                conditional_get,
                allow_overwrite,
                sftp_strict_host_key_check,
                sftp_known_hosts,
                sftp_private_key,
                sftp_private_key_passphrase,
                quiet: cli.quiet,
            })
            .await?;
        }
        Commands::Daemon {
            dir,
            session_file,
            daemonize,
            save_session_interval,
            rpc_port,
            max_download_limit,
            max_tries,
            retry_wait,
            min_split_size,
            lowest_speed_limit,
            max_file_not_found,
            all_proxy,
            http_proxy,
            https_proxy,
            no_proxy,
            check_certificate,
            ca_certificate,
            bt_dht_config_file,
            bt_piece_strategy,
            certificate,
            private_key,
            user_agent,
            http_user,
            http_passwd,
            input_file,
            on_download_start,
            on_download_complete,
            on_download_error,
            rpc_secret,
            rpc_allow_origin_all,
            load_cookies,
            save_cookies,
            file_allocation,
            max_redirect,
            netrc_path,
            no_netrc,
            header,
            timeout,
            connect_timeout,
            conditional_get,
            allow_overwrite,
            sftp_strict_host_key_check,
            sftp_known_hosts,
            sftp_private_key,
            sftp_private_key_passphrase,
        } => {
            let mut config = base_config.clone();
            config.dir = dir.clone();
            config.max_concurrent_downloads = cli.max_concurrent;
            config.max_overall_download_limit = max_download_limit;
            config.quiet = cli.quiet;
            config.rpc_listen_port = rpc_port;
            config.enable_rpc = true;
            config.session_file = session_file.clone();
            if let Some(max_tries) = max_tries {
                config.max_tries = max_tries;
            }
            if let Some(retry_wait) = retry_wait {
                config.retry_wait = retry_wait;
            }
            if let Some(min_split_size) = min_split_size {
                config.min_split_size = min_split_size;
            }
            if let Some(lowest_speed_limit) = lowest_speed_limit {
                config.lowest_speed_limit = lowest_speed_limit;
            }
            if let Some(max_file_not_found) = max_file_not_found {
                config.max_file_not_found = max_file_not_found;
            }
            if save_session_interval.is_some() {
                config.save_session_interval = save_session_interval;
            }
            if all_proxy.is_some() {
                config.all_proxy = all_proxy;
            }
            if http_proxy.is_some() {
                config.http_proxy = http_proxy;
            }
            if https_proxy.is_some() {
                config.https_proxy = https_proxy;
            }
            if no_proxy.is_some() {
                config.no_proxy = no_proxy;
            }
            config.check_certificate = check_certificate;
            if ca_certificate.is_some() {
                config.ca_certificate = ca_certificate;
            }
            if bt_dht_config_file.is_some() {
                config.bt_dht_config_file = bt_dht_config_file;
            }
            if let Some(bt_piece_strategy) = bt_piece_strategy {
                config.bt_piece_strategy =
                    raria_core::config::BtPieceStrategy::parse(&bt_piece_strategy).ok_or_else(
                        || {
                            anyhow::anyhow!(
                                "invalid --bt-piece-strategy '{}': expected 'current' or 'rarest-first'",
                                bt_piece_strategy
                            )
                        },
                    )?;
            }
            if certificate.is_some() {
                config.certificate = certificate;
            }
            if private_key.is_some() {
                config.private_key = private_key;
            }
            if user_agent.is_some() {
                config.user_agent = user_agent;
            }
            if http_user.is_some() {
                config.http_user = http_user;
            }
            if http_passwd.is_some() {
                config.http_passwd = http_passwd;
            }
            if on_download_start.is_some() {
                config.on_download_start = on_download_start;
            }
            if on_download_complete.is_some() {
                config.on_download_complete = on_download_complete;
            }
            if on_download_error.is_some() {
                config.on_download_error = on_download_error;
            }
            if rpc_secret.is_some() {
                config.rpc_secret = rpc_secret;
            }
            config.rpc_allow_origin_all = rpc_allow_origin_all;
            if load_cookies.is_some() {
                config.cookie_file = load_cookies;
            }
            if save_cookies.is_some() {
                config.save_cookie_file = save_cookies;
            }
            if max_redirect.is_some() {
                config.max_redirects = max_redirect;
            }
            if netrc_path.is_some() {
                config.netrc_path = netrc_path;
            }
            config.no_netrc = no_netrc;
            config.timeout = timeout;
            config.connect_timeout = connect_timeout;
            config.conditional_get = conditional_get;
            config.allow_overwrite = allow_overwrite;
            config.sftp_strict_host_key_check = sftp_strict_host_key_check;
            if sftp_known_hosts.is_some() {
                config.sftp_known_hosts = sftp_known_hosts;
            }
            if sftp_private_key.is_some() {
                config.sftp_private_key = sftp_private_key;
            }
            if sftp_private_key_passphrase.is_some() {
                config.sftp_private_key_passphrase = sftp_private_key_passphrase;
            }
            config.file_allocation =
                raria_core::file_alloc::FileAllocation::parse(&file_allocation)?;

            let input_entries = if let Some(ref path) = input_file {
                let entries = raria_core::input_file::load_input_file_entries(path)?;
                info!(
                    count = entries.len(),
                    path = %path.display(),
                    "loaded URIs from input file"
                );
                entries
            } else {
                Vec::new()
            };

            let _ = daemonize;

            daemon::run_daemon_with_config(
                config,
                &session_file,
                input_entries,
                dir.clone(),
                header,
            )
            .await?;
        }
    }

    Ok(())
}
