mod backend_factory;
mod bt_runtime;
mod daemon;
mod executor_config;
mod hooks;
mod single;
mod util;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use raria_core::config::GlobalConfig;
use std::ffi::OsString;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[cfg(unix)]
fn spawn_background_daemon(raw_args: &[OsString]) -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let filtered_args: Vec<OsString> = raw_args
        .iter()
        .skip(1)
        .filter(|arg| *arg != "--detach")
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

#[cfg(test)]
mod tests {
    use super::{Cli, Commands};
    use clap::{CommandFactory, Parser};

    #[test]
    fn daemon_accepts_native_api_port_name() {
        let cli = Cli::try_parse_from(["raria", "daemon", "--api-port", "7777"])
            .expect("parse native API port");
        let Commands::Daemon { api_port, .. } = cli.command else {
            panic!("expected daemon command");
        };
        assert_eq!(api_port, 7777);
    }

    #[test]
    fn daemon_accepts_native_task_hook_names() {
        let cli = Cli::try_parse_from([
            "raria",
            "daemon",
            "--on-task-start",
            "/tmp/start.sh",
            "--on-task-complete",
            "/tmp/complete.sh",
            "--on-task-fail",
            "/tmp/fail.sh",
        ])
        .expect("parse native task hook names");
        let Commands::Daemon {
            on_task_start,
            on_task_complete,
            on_task_fail,
            ..
        } = cli.command
        else {
            panic!("expected daemon command");
        };
        assert_eq!(
            on_task_start.unwrap(),
            std::path::PathBuf::from("/tmp/start.sh")
        );
        assert_eq!(
            on_task_complete.unwrap(),
            std::path::PathBuf::from("/tmp/complete.sh")
        );
        assert_eq!(
            on_task_fail.unwrap(),
            std::path::PathBuf::from("/tmp/fail.sh")
        );
    }

    #[test]
    fn daemon_accepts_native_lifecycle_shutdown_policy_names() {
        let cli = Cli::try_parse_from([
            "raria",
            "daemon",
            "--stop-after",
            "60",
            "--stop-when-parent-exits",
            "12345",
        ])
        .expect("parse native daemon lifecycle names");
        let Commands::Daemon {
            stop_after,
            stop_when_parent_exits,
            ..
        } = cli.command
        else {
            panic!("expected daemon command");
        };
        assert_eq!(stop_after, Some(60));
        assert_eq!(stop_when_parent_exits, Some(12345));
    }

    #[test]
    fn download_accepts_ed2k_links_as_native_urls() {
        let link = "ed2k://|file|sample.iso|1234|0123456789abcdef0123456789abcdef|/";
        let cli = Cli::try_parse_from(["raria", "download", link, "--filename", "sample.iso"])
            .expect("parse ED2K download URL");
        let Commands::Download { url, out, .. } = cli.command else {
            panic!("expected download command");
        };
        assert_eq!(url, link);
        assert_eq!(out.as_deref(), Some("sample.iso"));
    }

    #[test]
    fn help_exposes_native_cli_names_only() {
        let mut command = Cli::command();
        let mut help = command.render_long_help().to_string();
        for subcommand in ["download", "daemon", "completion"] {
            help.push_str(
                &command
                    .find_subcommand_mut(subcommand)
                    .expect("subcommand")
                    .render_long_help()
                    .to_string(),
            );
        }
        let tokens = help
            .split_whitespace()
            .map(|token| token.trim_end_matches(','))
            .collect::<Vec<_>>();

        for native in [
            "--config",
            "--download-dir",
            "--filename",
            "--segments",
            "--resume",
            "--proxy",
            "--http-username",
            "--http-password",
            "--task-file",
            "--detach",
            "completion",
        ] {
            assert!(tokens.contains(&native), "missing native flag {native}");
        }
        for native_value in [
            "<CONFIG>",
            "<DOWNLOAD_DIR>",
            "<FILENAME>",
            "<SEGMENTS>",
            "<DOWNLOAD_LIMIT>",
            "<RETRY_ATTEMPTS>",
            "<RETRY_DELAY>",
            "<MIN_SEGMENT_SIZE>",
            "<MIN_SPEED>",
            "<MAX_NOT_FOUND>",
            "<PROXY>",
            "<HTTP_USERNAME>",
            "<HTTP_PASSWORD>",
            "<TASK_FILE>",
        ] {
            assert!(
                tokens.contains(&native_value),
                "missing native value name {native_value}"
            );
        }
    }

    #[test]
    fn completion_accepts_shell_name() {
        let cli =
            Cli::try_parse_from(["raria", "completion", "bash"]).expect("parse completion shell");
        let Commands::Completion { shell } = cli.command else {
            panic!("expected completion command");
        };
        assert_eq!(shell, clap_complete::Shell::Bash);
    }
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

    /// Path to native raria.toml configuration file.
    #[arg(long = "config", value_name = "CONFIG", global = true)]
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
        #[arg(
            long = "download-dir",
            value_name = "DOWNLOAD_DIR",
            default_value = "."
        )]
        dir: PathBuf,

        /// Output filename (default: derived from URL)
        #[arg(long = "filename", value_name = "FILENAME")]
        out: Option<String>,

        /// Number of range segments.
        #[arg(long = "segments", value_name = "SEGMENTS", default_value_t = 16)]
        connections: u32,

        /// Continue downloading a partially downloaded file.
        #[arg(long = "resume", default_value_t = false)]
        resume: bool,

        /// Maximum download speed (bytes/sec, 0 = unlimited)
        #[arg(
            long = "download-limit",
            value_name = "DOWNLOAD_LIMIT",
            default_value_t = 0
        )]
        max_download_limit: u64,

        /// Maximum retry attempts per segment; 0 means unlimited retries.
        #[arg(long = "retry-attempts", value_name = "RETRY_ATTEMPTS")]
        retry_attempts: Option<u32>,

        /// Seconds to wait between retry attempts.
        #[arg(long = "retry-delay", value_name = "RETRY_DELAY")]
        retry_delay_seconds: Option<u32>,

        /// Minimum size in bytes for a segment.
        #[arg(long = "min-segment-size", value_name = "MIN_SEGMENT_SIZE")]
        min_segment_size: Option<u64>,

        /// Abort connections when download speed is below this limit (bytes/sec).
        #[arg(long = "min-speed", value_name = "MIN_SPEED")]
        min_speed: Option<u64>,

        /// Maximum number of file-not-found errors before giving up.
        #[arg(long = "max-not-found", value_name = "MAX_NOT_FOUND")]
        max_not_found: Option<u32>,

        /// Path to Netscape cookie file for persistence.
        #[arg(long = "cookie-store-file", value_name = "COOKIE_STORE_FILE")]
        save_cookies: Option<PathBuf>,

        /// Checksum for verification (format: algo=hex, e.g. sha-256=abc...)
        #[arg(long)]
        checksum: Option<String>,

        /// Proxy URL for all protocols
        #[arg(long = "proxy", value_name = "PROXY")]
        proxy: Option<String>,

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
        #[arg(long = "http-username", value_name = "HTTP_USERNAME")]
        http_user: Option<String>,

        /// HTTP Basic auth password
        #[arg(long = "http-password", value_name = "HTTP_PASSWORD")]
        http_password: Option<String>,

        /// Maximum number of redirects to follow (0 disables redirects)
        #[arg(long = "redirect-limit", value_name = "REDIRECT_LIMIT")]
        max_redirect: Option<usize>,

        /// Path to a netrc file for host credential lookup
        #[arg(long = "netrc-file", value_name = "NETRC_FILE")]
        netrc_path: Option<PathBuf>,

        /// Disable all netrc credential loading
        #[arg(long = "disable-netrc", default_value_t = false)]
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

    /// Run as a persistent daemon with native API server.
    Daemon {
        /// Output directory for downloads
        #[arg(
            long = "download-dir",
            value_name = "DOWNLOAD_DIR",
            default_value = "."
        )]
        dir: PathBuf,

        /// Session file for persistence
        #[arg(
            long = "session-path",
            value_name = "SESSION_PATH",
            default_value = "raria.session.redb"
        )]
        session_file: PathBuf,

        /// Detach and keep the daemon running in the background.
        #[arg(long = "detach", default_value_t = false)]
        daemonize: bool,

        /// Save the current session periodically while running.
        #[arg(long)]
        save_session_interval: Option<u64>,

        /// Native API listen port.
        #[arg(long = "api-port", default_value_t = 6800)]
        api_port: u16,

        /// Maximum download speed (bytes/sec, 0 = unlimited)
        #[arg(
            long = "download-limit",
            value_name = "DOWNLOAD_LIMIT",
            default_value_t = 0
        )]
        max_download_limit: u64,

        /// Maximum retry attempts per segment; 0 means unlimited retries.
        #[arg(long = "retry-attempts", value_name = "RETRY_ATTEMPTS")]
        retry_attempts: Option<u32>,

        /// Seconds to wait between retry attempts.
        #[arg(long = "retry-delay", value_name = "RETRY_DELAY")]
        retry_delay_seconds: Option<u32>,

        /// Minimum size in bytes for a segment.
        #[arg(long = "min-segment-size", value_name = "MIN_SEGMENT_SIZE")]
        min_segment_size: Option<u64>,

        /// Abort connections when download speed is below this limit (bytes/sec).
        #[arg(long = "min-speed", value_name = "MIN_SPEED")]
        min_speed: Option<u64>,

        /// Maximum number of file-not-found errors before giving up.
        #[arg(long = "max-not-found", value_name = "MAX_NOT_FOUND")]
        max_not_found: Option<u32>,

        /// Proxy URL for all protocols
        #[arg(long = "proxy", value_name = "PROXY")]
        proxy: Option<String>,

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
        #[arg(long = "http-username", value_name = "HTTP_USERNAME")]
        http_user: Option<String>,

        /// HTTP Basic auth password
        #[arg(long = "http-password", value_name = "HTTP_PASSWORD")]
        http_password: Option<String>,

        /// Input file containing URIs to download (one per line)
        #[arg(long = "task-file", value_name = "TASK_FILE")]
        input_file: Option<PathBuf>,

        /// Hook script fired when a task starts running.
        #[arg(long = "on-task-start")]
        on_task_start: Option<PathBuf>,

        /// Hook script fired when a task completes.
        #[arg(long = "on-task-complete")]
        on_task_complete: Option<PathBuf>,

        /// Hook script fired when a task fails.
        #[arg(long = "on-task-fail")]
        on_task_fail: Option<PathBuf>,

        /// Path to Netscape cookie file
        #[arg(long = "cookie-file", value_name = "COOKIE_FILE")]
        load_cookies: Option<PathBuf>,

        /// Path to Netscape cookie file for persistence
        #[arg(long = "cookie-store-file", value_name = "COOKIE_STORE_FILE")]
        save_cookies: Option<PathBuf>,

        /// File allocation strategy: none, prealloc, trunc, falloc
        #[arg(long, default_value = "none")]
        file_allocation: String,

        /// Maximum number of redirects to follow (0 disables redirects)
        #[arg(long = "redirect-limit", value_name = "REDIRECT_LIMIT")]
        max_redirect: Option<usize>,

        /// Path to a netrc file for host credential lookup
        #[arg(long = "netrc-file", value_name = "NETRC_FILE")]
        netrc_path: Option<PathBuf>,

        /// Disable all netrc credential loading
        #[arg(long = "disable-netrc", default_value_t = false)]
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

        /// Stop the daemon after this many seconds.
        #[arg(long = "stop-after", value_name = "STOP_AFTER")]
        stop_after: Option<u64>,

        /// Stop the daemon when this parent process exits.
        #[arg(long = "stop-when-parent-exits", value_name = "PARENT_PID")]
        stop_when_parent_exits: Option<u32>,
    },

    /// Generate native shell completion for raria.
    Completion {
        /// Target shell.
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    #[allow(unused_variables)]
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let cli = Cli::parse();

    let completion_shell = match &cli.command {
        Commands::Completion { shell } => Some(*shell),
        _ => None,
    };
    if let Some(shell) = completion_shell {
        let mut command = Cli::command();
        let command_name = command.get_name().to_string();
        generate(shell, &mut command, command_name, &mut std::io::stdout());
        return Ok(());
    }

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
        match raria_core::native_config::RariaConfig::from_toml_file(conf_path)
            .and_then(|config| config.to_global_config())
        {
            Ok(config) => {
                base_config = config;
                info!(path = %conf_path.display(), "loaded native raria configuration file");
            }
            Err(e) => {
                error!(
                    path = %conf_path.display(), error = %e,
                    "invalid native raria configuration"
                );
                std::process::exit(1);
            }
        }
    }

    match cli.command {
        Commands::Download {
            url,
            dir,
            out,
            connections,
            resume,
            max_download_limit,
            retry_attempts,
            retry_delay_seconds,
            min_segment_size,
            min_speed,
            max_not_found,
            save_cookies,
            checksum,
            proxy,
            check_certificate,
            ca_certificate,
            certificate,
            private_key,
            user_agent,
            http_user,
            http_password,
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
                resume,
                max_concurrent: cli.max_concurrent,
                max_download_limit,
                retry_attempts,
                retry_delay_seconds,
                min_segment_size,
                min_speed,
                max_not_found,
                save_cookies,
                checksum_spec: checksum,
                proxy,
                check_certificate: check_certificate.unwrap_or(true),
                ca_certificate,
                certificate,
                private_key,
                user_agent,
                http_user,
                http_password,
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
            api_port,
            max_download_limit,
            retry_attempts,
            retry_delay_seconds,
            min_segment_size,
            min_speed,
            max_not_found,
            proxy,
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
            http_password,
            input_file,
            on_task_start,
            on_task_complete,
            on_task_fail,
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
            stop_after,
            stop_when_parent_exits,
        } => {
            let mut config = base_config.clone();
            config.download_dir = dir.clone();
            config.max_concurrent_downloads = cli.max_concurrent;
            config.global_download_limit = max_download_limit;
            config.quiet = cli.quiet;
            config.api_listen_port = api_port;
            config.session_file = session_file.clone();
            if let Some(retry_attempts) = retry_attempts {
                config.retry_attempts = retry_attempts;
            }
            if let Some(retry_delay_seconds) = retry_delay_seconds {
                config.retry_delay_seconds = retry_delay_seconds;
            }
            if let Some(min_segment_size) = min_segment_size {
                config.min_segment_size = min_segment_size;
            }
            if let Some(min_speed) = min_speed {
                config.min_speed = min_speed;
            }
            if let Some(max_not_found) = max_not_found {
                config.max_not_found = max_not_found;
            }
            if save_session_interval.is_some() {
                config.save_session_interval = save_session_interval;
            }
            if proxy.is_some() {
                config.proxy = proxy;
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
                config.bt_piece_strategy = raria_core::config::BtPieceStrategy::parse(
                    &bt_piece_strategy,
                )
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid --bt-piece-strategy '{}': expected 'current' or 'rarest-first'",
                        bt_piece_strategy
                    )
                })?;
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
            if http_password.is_some() {
                config.http_password = http_password;
            }
            if on_task_start.is_some() {
                config.on_task_start = on_task_start;
            }
            if on_task_complete.is_some() {
                config.on_task_complete = on_task_complete;
            }
            if on_task_fail.is_some() {
                config.on_task_fail = on_task_fail;
            }
            if load_cookies.is_some() {
                config.load_cookie_file = load_cookies;
            }
            if save_cookies.is_some() {
                config.cookie_store_file = save_cookies;
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
            config.daemon_stop_after_seconds = stop_after;
            config.daemon_parent_pid = stop_when_parent_exits;
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
        Commands::Completion { .. } => unreachable!("completion exits before logging setup"),
    }

    Ok(())
}
