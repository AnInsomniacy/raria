// raria-core: Configuration types.
//
// This module defines configuration structures for global and per-job settings.

use crate::file_alloc::FileAllocation;
use crate::native::NativeSourceHealth;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// BitTorrent piece selection strategy exposed through raria configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BtPieceStrategy {
    /// Prefer pieces that are available from the fewest live peers.
    #[default]
    RarestFirst,
    /// Keep librqbit's existing selection order.
    Current,
}

impl BtPieceStrategy {
    /// Parse the stable string form used by config files and CLI flags.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "current" => Some(Self::Current),
            "rarest-first" => Some(Self::RarestFirst),
            _ => None,
        }
    }

    /// Return the canonical config/CLI string representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::RarestFirst => "rarest-first",
        }
    }
}

/// Global configuration for the raria daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Default download directory.
    pub download_dir: PathBuf,
    /// Maximum number of concurrent downloads.
    pub max_concurrent_downloads: u32,
    /// Maximum global download speed in bytes/sec (0 = unlimited).
    pub global_download_limit: u64,
    /// Maximum global upload speed in bytes/sec (0 = unlimited).
    pub global_upload_limit: u64,
    /// Native API listen port.
    pub api_listen_port: u16,
    /// Path to the session file for persistence.
    pub session_file: PathBuf,
    /// Save the current session periodically while the daemon is running.
    pub save_session_interval: Option<u64>,
    /// Log level.
    pub log_level: String,
    /// Suppress normal user-facing output.
    pub quiet: bool,
    /// Proxy URL for all protocols.
    pub proxy: Option<String>,
    /// Proxy URL for HTTP requests only (overrides proxy for HTTP).
    pub http_proxy: Option<String>,
    /// Proxy URL for HTTPS requests only (overrides proxy for HTTPS).
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
    pub http_password: Option<String>,
    /// Path to Netscape cookie file loaded before HTTP requests.
    pub load_cookie_file: Option<PathBuf>,
    /// Path to Netscape cookie file updated after HTTP requests.
    pub cookie_store_file: Option<PathBuf>,
    /// Temporary JSON-RPC secret retained until the legacy server is deleted.
    pub rpc_secret: Option<String>,
    /// Native HTTP API bearer token.
    pub api_auth_token: Option<String>,
    /// Temporary JSON-RPC browser origin override retained until legacy deletion.
    pub rpc_allow_origin_all: bool,
    /// File allocation strategy.
    pub file_allocation: FileAllocation,
    /// Maximum connections per server.
    pub server_connection_limit: u32,
    /// Default segment count for range-capable downloads.
    pub default_segments: u32,
    /// Continue downloading a partially downloaded file.
    pub resume: bool,
    /// Minimum size in bytes for a segment.
    ///
    /// When set to a non-zero value, the effective number of connections for a
    /// range-capable download will be reduced so that each segment is at least
    /// this many bytes.
    pub min_segment_size: u64,
    /// Abort connections when download speed is below this limit (bytes/sec).
    /// 0 disables the check.
    pub min_speed: u64,
    /// Maximum number of file-not-found errors before giving up.
    /// 0 disables the check.
    pub max_not_found: u32,
    /// Maximum retries per download; 0 means unlimited retries.
    pub retry_attempts: u32,
    /// Seconds to wait between retries.
    pub retry_delay_seconds: u32,
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
    /// Preferred Metalink mirror locations.
    pub metalink_preferred_locations: Vec<String>,
    /// Preferred Metalink mirror protocol.
    pub metalink_preferred_protocol: Option<String>,
    /// Keep only the best Metalink source for each protocol.
    pub metalink_unique_protocols: bool,
    /// Optional BT DHT persistence/config file path used to seed librqbit's persistent DHT state.
    pub bt_dht_config_file: Option<PathBuf>,
    /// Enable BitTorrent peer exchange when the backend exposes it.
    pub bt_enable_pex: bool,
    /// BT piece selection strategy forwarded into the BitTorrent runtime.
    pub bt_piece_strategy: BtPieceStrategy,
    /// Hook script fired when a task starts running.
    pub on_task_start: Option<PathBuf>,
    /// Hook script fired when a task completes.
    pub on_task_complete: Option<PathBuf>,
    /// Hook script fired when a task fails.
    pub on_task_fail: Option<PathBuf>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::from("."),
            max_concurrent_downloads: 5,
            global_download_limit: 0,
            global_upload_limit: 0,
            api_listen_port: 6800,
            session_file: PathBuf::from("raria.session"),
            save_session_interval: None,
            log_level: "info".into(),
            quiet: false,
            proxy: None,
            http_proxy: None,
            https_proxy: None,
            no_proxy: None,
            check_certificate: true,
            ca_certificate: None,
            certificate: None,
            private_key: None,
            user_agent: None,
            http_user: None,
            http_password: None,
            load_cookie_file: None,
            cookie_store_file: None,
            rpc_secret: None,
            api_auth_token: None,
            rpc_allow_origin_all: false,
            file_allocation: FileAllocation::None,
            server_connection_limit: 16,
            default_segments: 5,
            resume: false,
            min_segment_size: 0,
            min_speed: 0,
            max_not_found: 0,
            retry_attempts: 5,
            retry_delay_seconds: 0,
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
            metalink_preferred_locations: Vec::new(),
            metalink_preferred_protocol: None,
            metalink_unique_protocols: false,
            bt_dht_config_file: None,
            bt_enable_pex: true,
            bt_piece_strategy: BtPieceStrategy::RarestFirst,
            on_task_start: None,
            on_task_complete: None,
            on_task_fail: None,
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
    pub http_password: Option<String>,
    /// Checksum for file verification (e.g., "sha-256=abc123").
    pub checksum: Option<String>,
    /// Zero-based BT file indices selected for download.
    pub bt_selected_files: Option<Vec<usize>>,
    /// Additional BT trackers appended to the torrent.
    pub bt_trackers: Option<Vec<String>>,
    /// Inspect BitTorrent metadata without starting payload transfer.
    pub bt_metadata_only: bool,
    /// Native BT tracker URIs excluded before task submission.
    pub bt_excluded_trackers: Vec<String>,
    /// Native BT tracker connect timeout in seconds.
    pub bt_tracker_connect_timeout_seconds: Option<u64>,
    /// Native BT tracker request timeout in seconds.
    pub bt_tracker_timeout_seconds: Option<u64>,
    /// Native BT tracker announce interval override in seconds.
    pub bt_tracker_interval_seconds: Option<u64>,
    /// Additional WebSeed URIs supplied alongside a torrent add request.
    pub bt_web_seed_uris: Option<Vec<String>>,
    /// Metadata source URIs carried from Metalink, keyed by media type.
    pub metalink_metadata_sources: Vec<MetalinkMetadataSource>,
    /// Delete unselected BitTorrent files after the selected payload completes.
    pub bt_delete_unselected_files_on_completion: bool,
    /// Stop seeding after this upload ratio is reached.
    pub seed_ratio: Option<f64>,
    /// Stop seeding after this many minutes.
    pub seed_time: Option<u64>,
    /// Stop an incomplete BitTorrent transfer after this many idle download seconds.
    pub bt_idle_download_timeout: Option<u64>,
    /// Runtime health recorded for task sources.
    pub source_health: HashMap<String, NativeSourceHealth>,
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
            http_password: None,
            checksum: None,
            bt_selected_files: None,
            bt_trackers: None,
            bt_metadata_only: false,
            bt_excluded_trackers: Vec::new(),
            bt_tracker_connect_timeout_seconds: None,
            bt_tracker_timeout_seconds: None,
            bt_tracker_interval_seconds: None,
            bt_web_seed_uris: None,
            metalink_metadata_sources: Vec::new(),
            bt_delete_unselected_files_on_completion: false,
            seed_ratio: None,
            seed_time: None,
            bt_idle_download_timeout: None,
            source_health: HashMap::new(),
        }
    }
}

/// Metadata source associated with a Metalink file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MetalinkMetadataSource {
    /// Metadata source URI.
    pub uri: String,
    /// Metadata media type, such as `torrent`.
    pub media_type: String,
    /// Priority where lower values are preferred.
    pub priority: u32,
    /// Optional metadata name.
    pub name: Option<String>,
}

impl Default for MetalinkMetadataSource {
    fn default() -> Self {
        Self {
            uri: String::new(),
            media_type: String::new(),
            priority: 999999,
            name: None,
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
        assert_eq!(cfg.global_download_limit, 0);
        assert_eq!(cfg.api_listen_port, 6800);
        assert!(!cfg.rpc_allow_origin_all);
        assert_eq!(cfg.bt_piece_strategy, BtPieceStrategy::RarestFirst);
        assert!(cfg.bt_enable_pex);
    }

    #[test]
    fn global_config_serde_roundtrips() {
        let cfg = GlobalConfig {
            bt_piece_strategy: BtPieceStrategy::RarestFirst,
            ..GlobalConfig::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let recovered: GlobalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            recovered.max_concurrent_downloads,
            cfg.max_concurrent_downloads
        );
        assert_eq!(recovered.api_listen_port, cfg.api_listen_port);
        assert_eq!(recovered.bt_piece_strategy, BtPieceStrategy::RarestFirst);
        assert!(recovered.bt_enable_pex);
    }

    #[test]
    fn global_config_serialization_uses_native_field_names() {
        let json = serde_json::to_value(GlobalConfig::default()).unwrap();
        let fields = json.as_object().unwrap();

        for native in [
            "download_dir",
            "global_download_limit",
            "global_upload_limit",
            "proxy",
            "http_password",
            "load_cookie_file",
            "cookie_store_file",
            "default_segments",
            "resume",
            "min_segment_size",
            "min_speed",
            "max_not_found",
            "retry_attempts",
            "retry_delay_seconds",
        ] {
            assert!(fields.contains_key(native), "missing native field {native}");
        }

        for legacy in [
            "dir",
            "max_overall_download_limit",
            "max_overall_upload_limit",
            "all_proxy",
            "http_passwd",
            "cookie_file",
            "save_cookie_file",
            "split",
            "continue_download",
            "min_split_size",
            "lowest_speed_limit",
            "max_file_not_found",
            "max_tries",
            "retry_wait",
        ] {
            assert!(!fields.contains_key(legacy), "legacy field {legacy} leaked");
        }
    }

    #[test]
    fn bt_piece_strategy_parses_known_values() {
        assert_eq!(
            BtPieceStrategy::parse("current"),
            Some(BtPieceStrategy::Current)
        );
        assert_eq!(
            BtPieceStrategy::parse("rarest-first"),
            Some(BtPieceStrategy::RarestFirst)
        );
        assert_eq!(BtPieceStrategy::parse("unknown"), None);
    }

    #[test]
    fn job_options_default_values() {
        let opts = JobOptions::default();
        assert_eq!(opts.max_connections, 16);
        assert_eq!(opts.max_download_limit, 0);
        assert!(opts.headers.is_empty());
        assert!(opts.out.is_none());
        assert!(!opts.bt_metadata_only);
    }

    #[test]
    fn job_options_serde_roundtrips() {
        let mut opts = JobOptions::default();
        opts.headers
            .push(("Referer".into(), "https://example.com".into()));
        opts.out = Some("custom_name.zip".into());
        opts.bt_metadata_only = true;

        let json = serde_json::to_string(&opts).unwrap();
        let recovered: JobOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.headers.len(), 1);
        assert_eq!(recovered.out.as_deref(), Some("custom_name.zip"));
        assert!(recovered.bt_metadata_only);
    }
}
