// raria-core: Configuration types.
//
// This module defines configuration structures for global and per-job settings.

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
    /// Log level.
    pub log_level: String,
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
    /// User-Agent string override.
    pub user_agent: Option<String>,
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
            log_level: "info".into(),
            all_proxy: None,
            http_proxy: None,
            https_proxy: None,
            no_proxy: None,
            check_certificate: true,
            ca_certificate: None,
            user_agent: None,
        }
    }
}

/// Per-job options that override global defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    }

    #[test]
    fn global_config_serde_roundtrips() {
        let cfg = GlobalConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let recovered: GlobalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.max_concurrent_downloads, cfg.max_concurrent_downloads);
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
        opts.headers.push(("Referer".into(), "https://example.com".into()));
        opts.out = Some("custom_name.zip".into());

        let json = serde_json::to_string(&opts).unwrap();
        let recovered: JobOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.headers.len(), 1);
        assert_eq!(recovered.out.as_deref(), Some("custom_name.zip"));
    }
}
