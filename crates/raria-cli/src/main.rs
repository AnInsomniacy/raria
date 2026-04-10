mod backend_factory;
mod bt_runtime;
mod daemon;
mod single;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use raria_core::config::GlobalConfig;
use std::path::PathBuf;
use tracing::{info, warn};
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

    /// Suppress normal user-facing output
    #[arg(long, short = 'q', default_value_t = false, global = true)]
    quiet: bool,

    /// Path to configuration file (aria2-compatible format)
    #[arg(long, global = true)]
    conf_path: Option<PathBuf>,
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

        /// Proxy URL for all protocols
        #[arg(long)]
        all_proxy: Option<String>,

        /// Disable TLS certificate verification
        #[arg(long)]
        check_certificate: Option<bool>,

        /// Custom user-agent string
        #[arg(long)]
        user_agent: Option<String>,

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

        /// RPC listen port
        #[arg(long, default_value_t = 6800)]
        rpc_port: u16,

        /// Maximum download speed (bytes/sec, 0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_download_limit: u64,

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

        /// RPC secret token for authentication
        #[arg(long)]
        rpc_secret: Option<String>,

        /// Path to Netscape cookie file
        #[arg(long)]
        load_cookies: Option<PathBuf>,

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
    let cli = Cli::parse();

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cli.log_level));
    let subscriber = tracing_subscriber::fmt().with_env_filter(env_filter);
    if cli.quiet {
        subscriber.with_writer(std::io::sink).init();
    } else {
        subscriber.init();
    }

    let mut base_config = GlobalConfig::default();
    if let Some(ref conf_path) = cli.conf_path {
        use raria_core::config_file::load_config_file;
        match load_config_file(&mut base_config, conf_path) {
            Ok(()) => info!(path = %conf_path.display(), "loaded configuration file"),
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
            max_download_limit,
            checksum,
            all_proxy,
            check_certificate,
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
            single::run_download(
                &url,
                &dir,
                out,
                connections,
                cli.max_concurrent,
                max_download_limit,
                checksum,
                all_proxy,
                check_certificate.unwrap_or(true),
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
                cli.quiet,
            )
            .await?;
        }
        Commands::Daemon {
            dir,
            session_file,
            rpc_port,
            max_download_limit,
            all_proxy,
            http_proxy,
            https_proxy,
            no_proxy,
            check_certificate,
            ca_certificate,
            user_agent,
            http_user,
            http_passwd,
            input_file,
            rpc_secret,
            load_cookies,
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
            if user_agent.is_some() {
                config.user_agent = user_agent;
            }
            if http_user.is_some() {
                config.http_user = http_user;
            }
            if http_passwd.is_some() {
                config.http_passwd = http_passwd;
            }
            if rpc_secret.is_some() {
                config.rpc_secret = rpc_secret;
            }
            if load_cookies.is_some() {
                config.cookie_file = load_cookies;
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
            config.file_allocation = raria_core::file_alloc::FileAllocation::parse(&file_allocation)?;

            let input_uris = if let Some(ref path) = input_file {
                let uris = raria_core::input_file::load_input_file(path)?;
                info!(count = uris.len(), path = %path.display(), "loaded URIs from input file");
                uris
            } else {
                Vec::new()
            };

            daemon::run_daemon_with_config(
                config,
                &session_file,
                input_uris,
                dir.clone(),
                header,
            )
            .await?;
        }
    }

    Ok(())
}
