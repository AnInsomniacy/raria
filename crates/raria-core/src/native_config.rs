//! Native `raria.toml` configuration schema.

use crate::config::GlobalConfig;
use crate::file_alloc::FileAllocation;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level native raria configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RariaConfig {
    /// Daemon process settings.
    #[serde(default)]
    pub daemon: DaemonConfig,
    /// Native HTTP API settings.
    #[serde(default)]
    pub api: ApiConfig,
    /// Default download behavior.
    #[serde(default)]
    pub downloads: DownloadsConfig,
    /// Shared network settings.
    #[serde(default)]
    pub network: NetworkConfig,
    /// BitTorrent settings.
    #[serde(default)]
    pub bittorrent: BitTorrentConfig,
    /// Local storage settings.
    #[serde(default)]
    pub storage: StorageConfig,
    /// Logging settings.
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl RariaConfig {
    /// Parse strict native TOML configuration.
    pub fn from_toml_str(input: &str) -> anyhow::Result<Self> {
        toml::from_str(input).map_err(Into::into)
    }

    /// Load strict native TOML configuration from a file.
    pub fn from_toml_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    /// Convert native configuration into the current runtime configuration.
    pub fn to_global_config(&self) -> anyhow::Result<GlobalConfig> {
        let mut config = GlobalConfig {
            dir: self.daemon.download_dir.clone(),
            session_file: self.daemon.session_path.clone(),
            max_concurrent_downloads: self.daemon.max_active_tasks,
            split: self.downloads.default_segments,
            min_split_size: self.downloads.min_segment_size,
            max_tries: self.downloads.retry_max_attempts,
            all_proxy: self.network.proxy.clone(),
            no_proxy: if self.network.no_proxy.is_empty() {
                None
            } else {
                Some(self.network.no_proxy.join(","))
            },
            enable_rpc: true,
            file_allocation: self.storage.file_allocation.to_runtime(),
            ..GlobalConfig::default()
        };

        let listen_addr: std::net::SocketAddr = self.api.listen_addr.parse()?;
        config.rpc_listen_port = listen_addr.port();
        config.api_auth_token = self.api_auth_token()?;
        Ok(config)
    }

    /// Load the configured native API bearer token, if one is configured.
    pub fn api_auth_token(&self) -> anyhow::Result<Option<String>> {
        let Some(path) = self.api.auth_token_file.as_deref() else {
            return Ok(None);
        };
        let token = std::fs::read_to_string(path)?;
        Ok(Some(token.trim().to_string()))
    }
}

/// Daemon process settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DaemonConfig {
    /// Default directory for downloaded files.
    pub download_dir: PathBuf,
    /// Native redb session store path.
    pub session_path: PathBuf,
    /// Maximum number of tasks allowed to run at once.
    pub max_active_tasks: u32,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::from("."),
            session_path: PathBuf::from("raria.session.redb"),
            max_active_tasks: 5,
        }
    }
}

/// Native HTTP API settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApiConfig {
    /// API listen address in `host:port` form.
    pub listen_addr: String,
    /// Allowed browser origins.
    pub allow_origins: Vec<String>,
    /// Optional file containing the API bearer token.
    pub auth_token_file: Option<PathBuf>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:6800".to_string(),
            allow_origins: Vec::new(),
            auth_token_file: None,
        }
    }
}

/// Default download behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DownloadsConfig {
    /// Default segment count for range-capable downloads.
    pub default_segments: u32,
    /// Minimum segment size in bytes.
    pub min_segment_size: u64,
    /// Maximum retry attempts per task.
    pub retry_max_attempts: u32,
}

impl Default for DownloadsConfig {
    fn default() -> Self {
        Self {
            default_segments: 5,
            min_segment_size: 0,
            retry_max_attempts: 5,
        }
    }
}

/// Shared network settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    /// Proxy URI for outbound connections.
    pub proxy: Option<String>,
    /// Hosts or domains that bypass proxy settings.
    pub no_proxy: Vec<String>,
}

/// BitTorrent settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BitTorrentConfig {
    /// Enable DHT.
    pub enable_dht: bool,
    /// Enable UDP trackers.
    pub enable_udp_trackers: bool,
    /// Enable peer exchange when the backend supports it.
    pub enable_pex: bool,
    /// Optional seed ratio limit.
    pub seed_ratio: Option<f64>,
    /// Optional seed time limit in minutes.
    pub seed_time: Option<u64>,
}

impl Default for BitTorrentConfig {
    fn default() -> Self {
        Self {
            enable_dht: true,
            enable_udp_trackers: true,
            enable_pex: true,
            seed_ratio: None,
            seed_time: None,
        }
    }
}

/// Local storage settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// File allocation strategy.
    pub file_allocation: FileAllocationMode,
    /// Existing-file conflict policy.
    pub conflict_policy: ConflictPolicy,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            file_allocation: FileAllocationMode::None,
            conflict_policy: ConflictPolicy::Rename,
        }
    }
}

/// Native file allocation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileAllocationMode {
    /// Do not preallocate files.
    None,
    /// Preallocate files before transfer.
    Prealloc,
    /// Truncate output files to expected length.
    Trunc,
    /// Use platform fallocate support when available.
    Falloc,
}

impl FileAllocationMode {
    /// Stable config string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Prealloc => "prealloc",
            Self::Trunc => "trunc",
            Self::Falloc => "falloc",
        }
    }

    fn to_runtime(self) -> FileAllocation {
        match self {
            Self::None => FileAllocation::None,
            Self::Prealloc => FileAllocation::Prealloc,
            Self::Trunc => FileAllocation::Trunc,
            Self::Falloc => FileAllocation::Falloc,
        }
    }
}

/// Native file conflict policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    /// Rename new output files on collision.
    Rename,
    /// Overwrite existing files.
    Overwrite,
    /// Reuse existing partial files when validators allow it.
    ReusePartial,
    /// Fail when output already exists.
    Fail,
}

/// Logging settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Structured JSONL log path.
    pub structured_log_path: Option<PathBuf>,
}
