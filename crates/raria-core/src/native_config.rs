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
    /// ED2K/eMule settings.
    #[serde(default)]
    pub ed2k: Ed2kConfig,
    /// Metalink settings.
    #[serde(default)]
    pub metalink: MetalinkConfig,
    /// Local storage settings.
    #[serde(default)]
    pub storage: StorageConfig,
    /// Lifecycle hook settings.
    #[serde(default)]
    pub hooks: HooksConfig,
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
            download_dir: self.daemon.download_dir.clone(),
            session_file: self.daemon.session_path.clone(),
            max_concurrent_downloads: self.daemon.max_active_tasks,
            default_segments: self.downloads.default_segments,
            min_segment_size: self.downloads.min_segment_size,
            retry_attempts: self.downloads.retry_max_attempts,
            proxy: self.network.proxy.clone(),
            no_proxy: if self.network.no_proxy.is_empty() {
                None
            } else {
                Some(self.network.no_proxy.join(","))
            },
            bt_enable_pex: self.bittorrent.enable_pex,
            metalink_preferred_locations: self.metalink.preferred_locations.clone(),
            metalink_preferred_protocol: self.metalink.preferred_protocol.clone(),
            metalink_unique_protocols: self.metalink.unique_protocols,
            ed2k_enabled: self.ed2k.enabled,
            ed2k_enable_servers: self.ed2k.enable_servers,
            ed2k_enable_kad: self.ed2k.enable_kad,
            ed2k_listen_tcp_port: self.ed2k.listen_tcp_port,
            ed2k_listen_udp_port: self.ed2k.listen_udp_port,
            ed2k_assume_firewalled: self.ed2k.assume_firewalled,
            ed2k_max_sources_per_task: self.ed2k.max_sources_per_task,
            ed2k_max_upload_slots: self.ed2k.max_upload_slots,
            ed2k_share_completed: self.ed2k.share_completed,
            file_allocation: self.storage.file_allocation.to_runtime(),
            auto_file_renaming: self.storage.conflict_policy.auto_file_renaming(),
            allow_overwrite: self.storage.conflict_policy.allow_overwrite(),
            on_task_start: self.hooks.task_started.clone(),
            on_task_complete: self.hooks.task_completed.clone(),
            on_task_fail: self.hooks.task_failed.clone(),
            daemon_stop_after_seconds: self.daemon.stop_after_seconds,
            daemon_parent_pid: self.daemon.stop_when_parent_exits,
            ..GlobalConfig::default()
        };

        let listen_addr: std::net::SocketAddr = self.api.listen_addr.parse()?;
        config.api_listen_port = listen_addr.port();
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
    /// Stop the daemon after this many seconds.
    pub stop_after_seconds: Option<u64>,
    /// Stop the daemon when this parent process exits.
    pub stop_when_parent_exits: Option<u32>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::from("."),
            session_path: PathBuf::from("raria.session.redb"),
            max_active_tasks: 5,
            stop_after_seconds: None,
            stop_when_parent_exits: None,
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

/// Native ED2K/eMule settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ed2kConfig {
    /// Enable the native ED2K backend.
    pub enabled: bool,
    /// Enable ED2K server discovery.
    pub enable_servers: bool,
    /// Enable eMule Kad discovery.
    pub enable_kad: bool,
    /// TCP listen port for ED2K peer sessions.
    pub listen_tcp_port: u16,
    /// UDP listen port for ED2K server UDP and Kad traffic.
    pub listen_udp_port: u16,
    /// Treat ED2K/Kad listen ports as firewalled until runtime evidence proves otherwise.
    pub assume_firewalled: bool,
    /// Maximum retained sources per ED2K task.
    pub max_sources_per_task: u32,
    /// Maximum local upload slots for shared ED2K files.
    pub max_upload_slots: u16,
    /// Share completed ED2K files through native metadata.
    pub share_completed: bool,
}

impl Default for Ed2kConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            enable_servers: true,
            enable_kad: true,
            listen_tcp_port: 4662,
            listen_udp_port: 4672,
            assume_firewalled: false,
            max_sources_per_task: 400,
            max_upload_slots: 3,
            share_completed: false,
        }
    }
}

/// Metalink settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MetalinkConfig {
    /// Preferred mirror locations in order.
    pub preferred_locations: Vec<String>,
    /// Preferred mirror protocol.
    pub preferred_protocol: Option<String>,
    /// Keep only the best source for each protocol after sorting.
    pub unique_protocols: bool,
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

impl ConflictPolicy {
    /// Whether this policy resolves an output collision by selecting a new name.
    pub const fn auto_file_renaming(self) -> bool {
        matches!(self, Self::Rename)
    }

    /// Whether this policy allows writing into an existing output path.
    pub const fn allow_overwrite(self) -> bool {
        matches!(self, Self::Overwrite | Self::ReusePartial)
    }
}

/// Lifecycle hook settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct HooksConfig {
    /// Script run when a task starts running.
    pub task_started: Option<PathBuf>,
    /// Script run when a task completes.
    pub task_completed: Option<PathBuf>,
    /// Script run when a task fails.
    pub task_failed: Option<PathBuf>,
}

/// Logging settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Structured JSONL log path.
    pub structured_log_path: Option<PathBuf>,
}
