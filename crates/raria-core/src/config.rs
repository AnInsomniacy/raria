// raria-core: Configuration types.
//
// This module defines configuration structures for global and per-job settings.

use crate::file_alloc::FileAllocation;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global configuration for the raria daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Default download directory.
    pub dir: PathBuf,
    /// Maximum number of concurrent downloads.
    pub max_concurrent_downloads: u32,
    /// Maximum global download speed in bytes/sec (0 = unlimited).
    pub max_overall_download_limit: u64,
    /// Maximum global upload speed in bytes/sec (0 = unlimited).
    pub max_overall_upload_limit: u64,
    /// RPC listen port.
    pub rpc_listen_port: u16,
    /// Whether to enable RPC.
    pub enable_rpc: bool,
    /// Path to the session file for persistence.
    pub session_file: PathBuf,
    /// Save the current session periodically while the daemon is running.
    pub save_session_interval: Option<u64>,
    /// Log level.
    pub log_level: String,
    /// Suppress normal user-facing output.
    pub quiet: bool,
    /// Proxy URL for all protocols (aria2: --all-proxy).
    pub all_proxy: Option<String>,
    /// Proxy URL for HTTP requests only (overrides all_proxy for HTTP).
    pub http_proxy: Option<String>,
    /// Proxy URL for HTTPS requests only (overrides all_proxy for HTTPS).
    pub https_proxy: Option<String>,
    /// Comma-separated list of domains that bypass the proxy.
    pub no_proxy: Option<String>,
    /// Whether to disable TLS certificate verification.
    pub check_certificate: bool,
    /// Path to custom CA certificate file.
    pub ca_certificate: Option<PathBuf>,
    /// Path to client certificate chain for mTLS.
    pub certificate: Option<PathBuf>,
    /// Path to client private key for mTLS.
    pub private_key: Option<PathBuf>,
    /// User-Agent string override.
    pub user_agent: Option<String>,
    /// Global HTTP Basic auth username.
    pub http_user: Option<String>,
    /// Global HTTP Basic auth password.
    pub http_passwd: Option<String>,
    /// Path to Netscape cookie file (aria2: --load-cookies).
    pub cookie_file: Option<PathBuf>,
    /// Path to Netscape cookie file for persistence (aria2: --save-cookies).
    pub save_cookie_file: Option<PathBuf>,
    /// RPC secret token (aria2: --rpc-secret). When set, all RPC
    /// requests must include "token:<secret>" as the first parameter.
    pub rpc_secret: Option<String>,
    /// Allow browsers from any origin to call the HTTP JSON-RPC endpoint.
    pub rpc_allow_origin_all: bool,
    /// File allocation strategy (aria2: --file-allocation).
    pub file_allocation: FileAllocation,
    /// Max connections per server (aria2: --max-connection-per-server / -x).
    pub max_connection_per_server: u32,
    /// Number of segments for splitting (aria2: --split / -s).
    pub split: u32,
    /// Continue downloading a partially downloaded file (aria2: --continue / -c).
    pub continue_download: bool,
    /// Minimum size in bytes for a split segment (aria2: --min-split-size).
    ///
    /// When set to a non-zero value, the effective number of connections for a
    /// range-capable download will be reduced so that each segment is at least
    /// this many bytes.
    pub min_split_size: u64,
    /// Abort connections when download speed is below this limit (bytes/sec).
    /// 0 disables the check (aria2: --lowest-speed-limit).
    pub lowest_speed_limit: u64,
    /// Maximum number of file-not-found errors before giving up (aria2: --max-file-not-found).
    /// 0 disables the check.
    pub max_file_not_found: u32,
    /// Maximum retries per download (aria2: --max-tries, 0 = infinite).
    pub max_tries: u32,
    /// Seconds to wait between retries (aria2: --retry-wait).
    pub retry_wait: u32,
    /// Maximum number of HTTP redirects to follow.
    pub max_redirects: Option<usize>,
    /// Auto-rename output files on collision instead of overwriting them.
    pub auto_file_renaming: bool,
    /// Path to a netrc file for credential lookup.
    pub netrc_path: Option<PathBuf>,
    /// Disable all netrc credential loading.
    pub no_netrc: bool,
    /// Default timeout for HTTP requests in seconds.
    pub timeout: Option<u64>,
    /// Connection establishment timeout for HTTP requests in seconds.
    pub connect_timeout: Option<u64>,
    /// Only download when the remote resource is newer than the local file.
    pub conditional_get: bool,
    /// Allow existing output files to be overwritten.
    pub allow_overwrite: bool,
    /// Enable strict SFTP host key verification.
    pub sftp_strict_host_key_check: bool,
    /// Optional known_hosts path for SFTP host verification.
    pub sftp_known_hosts: Option<PathBuf>,
    /// Optional SSH private key path used for SFTP authentication.
    pub sftp_private_key: Option<PathBuf>,
    /// Optional SSH private key passphrase used for SFTP authentication.
    pub sftp_private_key_passphrase: Option<String>,
    /// Hook script fired when a download starts.
    pub on_download_start: Option<PathBuf>,
    /// Hook script fired when a download completes.
    pub on_download_complete: Option<PathBuf>,
    /// Hook script fired when a download errors.
    pub on_download_error: Option<PathBuf>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("."),
            max_concurrent_downloads: 5,
            max_overall_download_limit: 0,
            max_overall_upload_limit: 0,
            rpc_listen_port: 6800,
            enable_rpc: false,
            session_file: PathBuf::from("raria.session"),
            save_session_interval: None,
            log_level: "info".into(),
            quiet: false,
            all_proxy: None,
            http_proxy: None,
            https_proxy: None,
            no_proxy: None,
            check_certificate: true,
            ca_certificate: None,
            certificate: None,
            private_key: None,
            user_agent: None,
            http_user: None,
            http_passwd: None,
            cookie_file: None,
            save_cookie_file: None,
            rpc_secret: None,
            rpc_allow_origin_all: false,
            file_allocation: FileAllocation::None,
            max_connection_per_server: 16,
            split: 5,
            continue_download: false,
            min_split_size: 0,
            lowest_speed_limit: 0,
            max_file_not_found: 0,
            max_tries: 5,
            retry_wait: 0,
            max_redirects: None,
            auto_file_renaming: true,
            netrc_path: None,
            no_netrc: false,
            timeout: None,
            connect_timeout: None,
            conditional_get: false,
            allow_overwrite: false,
            sftp_strict_host_key_check: false,
            sftp_known_hosts: None,
            sftp_private_key: None,
            sftp_private_key_passphrase: None,
            on_download_start: None,
            on_download_complete: None,
            on_download_error: None,
        }
    }
}

/// Per-job options that override global defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct JobOptions {
    /// Maximum number of connections per server for this job.
    pub max_connections: u32,
    /// Maximum download speed for this job in bytes/sec (0 = unlimited).
    pub max_download_limit: u64,
    /// Maximum upload speed for this job in bytes/sec (0 = unlimited, BT only).
    pub max_upload_limit: u64,
    /// Output directory override.
    pub dir: Option<PathBuf>,
    /// Output filename override.
    pub out: Option<String>,
    /// Custom HTTP headers.
    pub headers: Vec<(String, String)>,
    /// HTTP user for Basic auth.
    pub http_user: Option<String>,
    /// HTTP password for Basic auth.
    pub http_passwd: Option<String>,
    /// Checksum for file verification (e.g., "sha-256=abc123").
    pub checksum: Option<String>,
    /// Zero-based BT file indices selected for download.
    pub bt_selected_files: Option<Vec<usize>>,
    /// Additional BT trackers appended to the torrent.
    pub bt_trackers: Option<Vec<String>>,
    /// Stop seeding after this upload ratio is reached.
    pub seed_ratio: Option<f64>,
    /// Stop seeding after this many minutes.
    pub seed_time: Option<u64>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            max_connections: 16,
            max_download_limit: 0,
            max_upload_limit: 0,
            dir: None,
            out: None,
            headers: Vec::new(),
            http_user: None,
            http_passwd: None,
            checksum: None,
            bt_selected_files: None,
            bt_trackers: None,
            seed_ratio: None,
            seed_time: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_config_default_values() {
        let cfg = GlobalConfig::default();
        assert_eq!(cfg.max_concurrent_downloads, 5);
        assert_eq!(cfg.max_overall_download_limit, 0);
        assert_eq!(cfg.rpc_listen_port, 6800);
        assert!(!cfg.enable_rpc);
        assert!(!cfg.rpc_allow_origin_all);
    }

    #[test]
    fn global_config_serde_roundtrips() {
        let cfg = GlobalConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let recovered: GlobalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            recovered.max_concurrent_downloads,
            cfg.max_concurrent_downloads
        );
        assert_eq!(recovered.rpc_listen_port, cfg.rpc_listen_port);
    }

    #[test]
    fn job_options_default_values() {
        let opts = JobOptions::default();
        assert_eq!(opts.max_connections, 16);
        assert_eq!(opts.max_download_limit, 0);
        assert!(opts.headers.is_empty());
        assert!(opts.out.is_none());
    }

    #[test]
    fn job_options_serde_roundtrips() {
        let mut opts = JobOptions::default();
        opts.headers
            .push(("Referer".into(), "https://example.com".into()));
        opts.out = Some("custom_name.zip".into());

        let json = serde_json::to_string(&opts).unwrap();
        let recovered: JobOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.headers.len(), 1);
        assert_eq!(recovered.out.as_deref(), Some("custom_name.zip"));
    }
}
