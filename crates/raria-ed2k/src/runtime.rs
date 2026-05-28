//! ED2K daemon runtime context and scheduling ownership.

use raria_core::config::GlobalConfig;
use raria_core::native::TaskId;
use std::collections::BTreeMap;
use std::time::Duration;

/// Native ED2K runtime configuration projected from `raria.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kRuntimeConfig {
    /// Whether server source discovery is enabled.
    pub enable_servers: bool,
    /// Whether Kad source discovery is enabled.
    pub enable_kad: bool,
    /// Local ED2K TCP listen port.
    pub listen_tcp_port: u16,
    /// Local ED2K UDP listen port.
    pub listen_udp_port: u16,
    /// Whether the runtime should start from a firewalled assumption.
    pub assume_firewalled: bool,
    /// Maximum retained sources per task.
    pub max_sources_per_task: u32,
    /// Maximum upload slots for shared files.
    pub max_upload_slots: u16,
    /// Whether completed files should enter native sharing.
    pub share_completed: bool,
}

impl Ed2kRuntimeConfig {
    /// Project native ED2K settings from the global daemon config.
    pub fn from_global_config(config: &GlobalConfig) -> Self {
        Self {
            enable_servers: config.ed2k_enable_servers,
            enable_kad: config.ed2k_enable_kad,
            listen_tcp_port: config.ed2k_listen_tcp_port,
            listen_udp_port: config.ed2k_listen_udp_port,
            assume_firewalled: config.ed2k_assume_firewalled,
            max_sources_per_task: config.ed2k_max_sources_per_task,
            max_upload_slots: config.ed2k_max_upload_slots,
            share_completed: config.ed2k_share_completed,
        }
    }
}

/// ED2K runtime event class before projection into raria-native event names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ed2kRuntimeEventKind {
    /// Source discovery state changed.
    Source,
    /// Peer queue state changed.
    Queue,
    /// Kad discovery state changed.
    Kad,
    /// Transfer state changed.
    Transfer,
    /// Sharing state changed.
    Sharing,
    /// Upload state changed.
    Upload,
}

/// Compact ED2K runtime status emitted by the runtime scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kRuntimeStatus {
    /// Runtime event class.
    pub event_kind: Ed2kRuntimeEventKind,
    /// Status category exposed in native events.
    pub category: &'static str,
    /// Stable state name exposed in native events.
    pub state: &'static str,
    /// Optional concise status message.
    pub message: Option<&'static str>,
    /// Numeric status metrics.
    pub metrics: BTreeMap<String, u64>,
}

/// Native ED2K runtime context for one raria task.
#[derive(Debug, Clone)]
pub struct Ed2kRuntimeContext {
    task_id: TaskId,
    identity_profile_id: String,
    config: Ed2kRuntimeConfig,
    state: Ed2kRuntimeState,
    scheduler_ticks: u64,
    last_tick_elapsed: Duration,
}

impl Ed2kRuntimeContext {
    /// Create a runtime context for one native ED2K task.
    pub fn new(task_id: TaskId, config: Ed2kRuntimeConfig) -> Self {
        Self {
            task_id,
            identity_profile_id: "default".to_string(),
            config,
            state: Ed2kRuntimeState::default(),
            scheduler_ticks: 0,
            last_tick_elapsed: Duration::ZERO,
        }
    }

    /// Return the native task id owned by this runtime context.
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    /// Return the native ED2K identity profile used by this runtime.
    pub fn identity_profile_id(&self) -> &str {
        &self.identity_profile_id
    }

    /// Return the projected runtime config.
    pub fn config(&self) -> &Ed2kRuntimeConfig {
        &self.config
    }

    /// Return the runtime-owned state snapshot.
    pub fn state(&self) -> &Ed2kRuntimeState {
        &self.state
    }

    /// Return startup statuses before any network loop runs.
    pub fn startup_statuses(&self) -> Vec<Ed2kRuntimeStatus> {
        vec![
            self.status(
                Ed2kRuntimeEventKind::Source,
                "source",
                if self.config.enable_servers || self.config.enable_kad {
                    "discovering"
                } else {
                    "disabled"
                },
                Some("ED2K runtime scheduler initialized"),
                [
                    ("knownSources", self.state.source.known_sources),
                    ("activeSources", self.state.source.active_sources),
                    ("schedulerTicks", self.scheduler_ticks),
                ],
            ),
            self.status(
                Ed2kRuntimeEventKind::Queue,
                "queue",
                "ready",
                None,
                [("waitingUploadPeers", self.state.queue.waiting_upload_peers)],
            ),
            self.status(
                Ed2kRuntimeEventKind::Kad,
                "kad",
                if self.config.enable_kad {
                    "discovering"
                } else {
                    "disabled"
                },
                None,
                [("knownKadContacts", self.state.kad.known_contacts)],
            ),
            self.status(
                Ed2kRuntimeEventKind::Sharing,
                "sharing",
                if self.config.share_completed {
                    "enabled"
                } else {
                    "disabled"
                },
                None,
                [("sharedFiles", self.state.sharing.shared_files)],
            ),
        ]
    }

    /// Advance bounded scheduler status for one elapsed timestamp.
    pub fn tick(&mut self, elapsed: Duration) -> Vec<Ed2kRuntimeStatus> {
        if elapsed == Duration::ZERO || elapsed <= self.last_tick_elapsed {
            return Vec::new();
        }
        self.last_tick_elapsed = elapsed;
        self.scheduler_ticks = self.scheduler_ticks.saturating_add(1);
        vec![
            self.status(
                Ed2kRuntimeEventKind::Source,
                "source",
                if self.config.enable_servers || self.config.enable_kad {
                    "discovering"
                } else {
                    "disabled"
                },
                None,
                [
                    ("knownSources", self.state.source.known_sources),
                    ("activeSources", self.state.source.active_sources),
                    ("schedulerTicks", self.scheduler_ticks),
                ],
            ),
            self.status(
                Ed2kRuntimeEventKind::Transfer,
                "transfer",
                "idle",
                None,
                [
                    ("activePeers", self.state.transfer.active_peers),
                    ("schedulerTicks", self.scheduler_ticks),
                ],
            ),
        ]
    }

    fn status<const N: usize>(
        &self,
        event_kind: Ed2kRuntimeEventKind,
        category: &'static str,
        state: &'static str,
        message: Option<&'static str>,
        metrics: [(&'static str, u64); N],
    ) -> Ed2kRuntimeStatus {
        Ed2kRuntimeStatus {
            event_kind,
            category,
            state,
            message,
            metrics: metrics
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        }
    }
}

/// Runtime-owned state for one ED2K task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kRuntimeState {
    /// Source discovery state.
    pub source: Ed2kSourceRuntimeState,
    /// Peer queue state.
    pub queue: Ed2kQueueRuntimeState,
    /// Kad discovery state.
    pub kad: Ed2kKadRuntimeState,
    /// Transfer worker state.
    pub transfer: Ed2kTransferRuntimeState,
    /// Sharing state.
    pub sharing: Ed2kSharingRuntimeState,
}

/// ED2K source discovery counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kSourceRuntimeState {
    /// Number of retained useful sources.
    pub known_sources: u64,
    /// Number of sources currently scheduled.
    pub active_sources: u64,
}

/// ED2K peer queue counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kQueueRuntimeState {
    /// Peers currently waiting for upload service.
    pub waiting_upload_peers: u64,
}

/// ED2K Kad counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kKadRuntimeState {
    /// Retained Kad contacts.
    pub known_contacts: u64,
}

/// ED2K transfer counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kTransferRuntimeState {
    /// Live peer transfer workers.
    pub active_peers: u64,
}

/// ED2K sharing counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kSharingRuntimeState {
    /// Files currently available through native ED2K sharing.
    pub shared_files: u64,
}
