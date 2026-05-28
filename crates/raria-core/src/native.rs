//! Native raria task and event model.

use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::PathBuf;

/// Public native identifier for a download task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    /// Generate a new opaque native task identifier.
    pub fn new() -> Self {
        let mut rng = rand::rng();
        let value: u128 = rng.random();
        Self(format!("task_{value:032x}"))
    }

    /// Borrow the string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse a task identifier received from a native public surface.
    pub fn parse(value: impl Into<String>) -> Result<Self, NativeModelError> {
        let value = value.into();
        let suffix = value
            .strip_prefix("task_")
            .ok_or(NativeModelError::InvalidTaskId)?;
        if suffix.len() == 32 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
            Ok(Self(value))
        } else {
            Err(NativeModelError::InvalidTaskId)
        }
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Native lifecycle state for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskLifecycle {
    /// Task is queued for execution.
    Queued,
    /// Task is actively transferring payload data.
    Running,
    /// Task is paused by user or policy.
    Paused,
    /// BitTorrent task is seeding after payload completion.
    Seeding,
    /// Task completed successfully.
    Completed,
    /// Task failed and is no longer retrying.
    Failed,
    /// Task was removed.
    Removed,
}

impl TaskLifecycle {
    /// Return the stable API string for this lifecycle state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Seeding => "seeding",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Removed => "removed",
        }
    }
}

/// Protocol detected for a task source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceProtocol {
    /// HTTP source.
    Http,
    /// HTTPS source.
    Https,
    /// FTP source.
    Ftp,
    /// FTPS source.
    Ftps,
    /// SFTP source.
    Sftp,
    /// BitTorrent magnet URI.
    Magnet,
    /// Local torrent file or torrent bytes reference.
    Torrent,
    /// Metalink document source.
    Metalink,
    /// Native ED2K/eMule link.
    Ed2k,
}

impl SourceProtocol {
    /// Detect a supported protocol from a URI-like source string.
    pub fn detect(uri: &str) -> Result<Self, NativeModelError> {
        if uri.starts_with("magnet:") {
            return Ok(Self::Magnet);
        }
        if uri.starts_with("torrent:") {
            return Ok(Self::Torrent);
        }
        if uri.ends_with(".torrent") {
            return Ok(Self::Torrent);
        }
        if uri.starts_with("metalink:") {
            return Ok(Self::Metalink);
        }
        if uri.starts_with("ed2k://") {
            return Ok(Self::Ed2k);
        }

        let parsed = url::Url::parse(uri).map_err(|_| NativeModelError::UnsupportedProtocol)?;
        match parsed.scheme() {
            "http" => Ok(Self::Http),
            "https" => Ok(Self::Https),
            "ftp" => Ok(Self::Ftp),
            "ftps" => Ok(Self::Ftps),
            "sftp" => Ok(Self::Sftp),
            _ => Err(NativeModelError::UnsupportedProtocol),
        }
    }
}

/// Native source projection for a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSource {
    /// Stable source identifier scoped to a task.
    pub id: String,
    /// Original source URI or opaque source reference.
    pub uri: String,
    /// Detected source protocol.
    pub protocol: SourceProtocol,
    /// User or document supplied source priority.
    pub priority: u32,
    /// Runtime health observed for this source.
    pub health: NativeSourceHealth,
}

impl TaskSource {
    /// Build a source projection from a URI-like input.
    pub fn new(uri: impl Into<String>) -> Result<Self, NativeModelError> {
        let uri = uri.into();
        let protocol = SourceProtocol::detect(&uri)?;
        let id = source_id(&uri);
        Ok(Self {
            id,
            uri,
            protocol,
            priority: 0,
            health: NativeSourceHealth::default(),
        })
    }

    /// Attach runtime health to a source projection.
    pub fn with_health(mut self, health: NativeSourceHealth) -> Self {
        self.health = health;
        self
    }
}

/// Runtime source health state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeSourceHealthState {
    /// No runtime observation has been recorded yet.
    Unknown,
    /// The source has completed a recent successful transfer.
    Healthy,
    /// The source has failed but is still eligible for future retry.
    Degraded,
    /// The source has failed enough times to be treated as the lowest scoring mirror.
    Failed,
}

impl NativeSourceHealthState {
    /// Return the stable API string for this state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

/// Runtime source health projection for native APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSourceHealth {
    /// Current source health state.
    pub state: NativeSourceHealthState,
    /// Number of failed runtime attempts recorded for this source.
    pub failure_count: u32,
    /// Last source-specific error, if any.
    pub last_error: Option<String>,
    /// Last measured transfer speed for this source, if any.
    pub last_download_bytes_per_second: Option<u64>,
    /// Relative selection score. Larger values are preferred.
    pub score: u64,
}

impl Default for NativeSourceHealth {
    fn default() -> Self {
        Self {
            state: NativeSourceHealthState::Unknown,
            failure_count: 0,
            last_error: None,
            last_download_bytes_per_second: None,
            score: 1_000,
        }
    }
}

impl NativeSourceHealth {
    /// Build a degraded or failed health projection from a failure counter.
    pub fn failed(failure_count: u32, last_error: impl Into<String>) -> Self {
        let state = if failure_count >= 3 {
            NativeSourceHealthState::Failed
        } else {
            NativeSourceHealthState::Degraded
        };
        let penalty = u64::from(failure_count).saturating_mul(250);
        Self {
            state,
            failure_count,
            last_error: Some(last_error.into()),
            last_download_bytes_per_second: None,
            score: 1_000_u64.saturating_sub(penalty),
        }
    }

    /// Build a healthy source projection from observed transfer speed.
    pub fn healthy(download_bytes_per_second: u64) -> Self {
        Self {
            state: NativeSourceHealthState::Healthy,
            failure_count: 0,
            last_error: None,
            last_download_bytes_per_second: Some(download_bytes_per_second),
            score: 2_000 + download_bytes_per_second,
        }
    }

    /// Whether this source has runtime observations.
    pub const fn is_unknown(&self) -> bool {
        matches!(self.state, NativeSourceHealthState::Unknown)
    }
}

/// Byte range using inclusive start and exclusive end offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    /// Inclusive start byte offset.
    pub start: u64,
    /// Exclusive end byte offset.
    pub end: u64,
}

impl ByteRange {
    /// Create a validated byte range.
    pub const fn new(start: u64, end: u64) -> Result<Self, NativeModelError> {
        if end < start {
            return Err(NativeModelError::InvalidByteRange);
        }
        Ok(Self { start, end })
    }

    /// Length of the byte range.
    pub const fn len(self) -> u64 {
        self.end - self.start
    }

    /// Whether this range is empty.
    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// Native WebSocket event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NativeEventType {
    /// Task was created.
    TaskCreated,
    /// Task started running.
    TaskStarted,
    /// Task was paused.
    TaskPaused,
    /// Task was resumed.
    TaskResumed,
    /// Task completed.
    TaskCompleted,
    /// Task failed.
    TaskFailed,
    /// Task was removed.
    TaskRemoved,
    /// Task progress changed.
    TaskProgress,
    /// One source failed while the task may continue.
    TaskSourceFailed,
    /// BitTorrent metadata was resolved.
    TaskBtMetadataResolved,
    /// BitTorrent payload completed and seeding started.
    TaskBtSeedingStarted,
    /// BitTorrent peer snapshot changed.
    TaskBtPeerUpdated,
    /// BitTorrent tracker snapshot changed.
    TaskBtTrackerUpdated,
    /// ED2K source state changed.
    TaskEd2kSourceUpdated,
    /// ED2K peer state changed.
    TaskEd2kPeerUpdated,
    /// ED2K queue state changed.
    TaskEd2kQueueUpdated,
    /// ED2K Kad state changed.
    TaskEd2kKadUpdated,
    /// ED2K transfer state changed.
    TaskEd2kTransferUpdated,
    /// ED2K sharing state changed.
    TaskEd2kSharingUpdated,
    /// ED2K upload state changed.
    TaskEd2kUploadUpdated,
    /// ED2K search state changed.
    TaskEd2kSearchUpdated,
}

impl Serialize for NativeEventType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl NativeEventType {
    /// Return the stable event type string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TaskCreated => "task.created",
            Self::TaskStarted => "task.started",
            Self::TaskPaused => "task.paused",
            Self::TaskResumed => "task.resumed",
            Self::TaskCompleted => "task.completed",
            Self::TaskFailed => "task.failed",
            Self::TaskRemoved => "task.removed",
            Self::TaskProgress => "task.progress",
            Self::TaskSourceFailed => "task.source.failed",
            Self::TaskBtMetadataResolved => "task.bt.metadata.resolved",
            Self::TaskBtSeedingStarted => "task.bt.seeding.started",
            Self::TaskBtPeerUpdated => "task.bt.peer.updated",
            Self::TaskBtTrackerUpdated => "task.bt.tracker.updated",
            Self::TaskEd2kSourceUpdated => "task.ed2k.source.updated",
            Self::TaskEd2kPeerUpdated => "task.ed2k.peer.updated",
            Self::TaskEd2kQueueUpdated => "task.ed2k.queue.updated",
            Self::TaskEd2kKadUpdated => "task.ed2k.kad.updated",
            Self::TaskEd2kTransferUpdated => "task.ed2k.transfer.updated",
            Self::TaskEd2kSharingUpdated => "task.ed2k.sharing.updated",
            Self::TaskEd2kUploadUpdated => "task.ed2k.upload.updated",
            Self::TaskEd2kSearchUpdated => "task.ed2k.search.updated",
        }
    }
}

/// Native event data payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NativeEventData {
    /// No additional data.
    Empty,
    /// Progress payload.
    Progress {
        /// Completed payload bytes.
        completed_bytes: u64,
        /// Total payload bytes, when known.
        total_bytes: Option<u64>,
        /// Current download speed in bytes per second.
        download_bytes_per_second: u64,
    },
    /// Error payload.
    Error {
        /// Stable raria error code.
        code: String,
        /// Human-readable message.
        message: String,
    },
    /// Source failure payload.
    SourceFailure {
        /// Source URI that failed.
        uri: String,
        /// Stable raria error code.
        code: String,
        /// Human-readable message.
        message: String,
    },
    /// BitTorrent metadata payload.
    BtMetadata {
        /// Torrent info hash.
        info_hash: String,
        /// Display name from torrent metadata.
        name: Option<String>,
        /// Total payload bytes.
        total_bytes: Option<u64>,
        /// Piece length in bytes.
        piece_length: Option<u64>,
        /// Number of pieces in the torrent.
        piece_count: Option<u64>,
    },
    /// BitTorrent seeding payload.
    BtSeeding {
        /// Bytes uploaded while seeding.
        uploaded_bytes: u64,
        /// Number of connected peers.
        peer_count: u32,
        /// Number of known seeders.
        seeder_count: Option<u32>,
    },
    /// BitTorrent peer update payload.
    BtPeer {
        /// Peer snapshot.
        peer: NativePeerSnapshot,
    },
    /// BitTorrent tracker update payload.
    BtTracker {
        /// Tracker snapshot.
        tracker: NativeTrackerSnapshot,
    },
    /// ED2K status payload.
    Ed2kStatus {
        /// ED2K subsystem category.
        category: String,
        /// Stable compact state.
        state: String,
        /// Optional human-readable detail.
        message: Option<String>,
        /// Numeric counters associated with the update.
        metrics: std::collections::BTreeMap<String, u64>,
    },
}

/// Versioned native WebSocket event envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeEvent {
    /// Event envelope schema version.
    pub version: u32,
    /// Monotonic stream sequence.
    pub sequence: u64,
    /// Event creation timestamp.
    pub time: DateTime<Utc>,
    /// Event type.
    #[serde(rename = "type")]
    pub event_type: NativeEventType,
    /// Related task, when the event is task-scoped.
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Event payload.
    pub data: NativeEventData,
}

impl NativeEvent {
    /// Create a native event with the current timestamp.
    pub fn new(
        sequence: u64,
        event_type: NativeEventType,
        task_id: Option<TaskId>,
        data: NativeEventData,
    ) -> Self {
        Self {
            version: 1,
            sequence,
            time: Utc::now(),
            event_type,
            task_id,
            data,
        }
    }
}

/// Metadata row for the native redb store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeStoreMetadata {
    /// Current store schema version.
    pub schema_version: u32,
    /// Stable identifier for this local store.
    pub store_id: String,
    /// Store creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last successful schema upgrade timestamp.
    pub last_schema_upgrade_at: Option<DateTime<Utc>>,
}

impl NativeStoreMetadata {
    /// Current native store schema version.
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;

    /// Create metadata for a native store.
    pub fn new(store_id: impl Into<String>) -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            store_id: store_id.into(),
            created_at: Utc::now(),
            last_schema_upgrade_at: None,
        }
    }
}

/// Versioned native task persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeTaskRow {
    /// Row schema version.
    pub row_version: u32,
    /// Public native task identifier.
    pub task_id: TaskId,
    /// Temporary runtime bridge id while the engine still uses numeric jobs internally.
    pub runtime_bridge_id: Option<u64>,
    /// Persisted lifecycle state.
    pub lifecycle: TaskLifecycle,
    /// Native backend kind required to restore the correct runtime executor.
    pub job_kind: crate::job::JobKind,
    /// Source URIs assigned to the task.
    pub sources: Vec<String>,
    /// Runtime health recorded for each source URI.
    #[serde(default)]
    pub source_health: std::collections::HashMap<String, NativeSourceHealth>,
    /// Primary output path.
    pub output_path: PathBuf,
    /// Total payload size, when known.
    pub total_bytes: Option<u64>,
    /// Completed payload bytes.
    pub completed_bytes: u64,
    /// Per-task segment/concurrency target.
    pub segments: u32,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Number of bytes in a native ED2K client identity.
pub const ED2K_CLIENT_IDENTITY_BYTES: usize = 16;

/// Versioned native ED2K identity persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kIdentityRow {
    /// Row schema version.
    pub row_version: u32,
    /// Native ED2K identity profile id.
    pub profile_id: String,
    /// Stable ED2K client hash.
    pub client_hash: [u8; ED2K_CLIENT_IDENTITY_BYTES],
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Versioned native ED2K server bootstrap persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kServerBootstrapRow {
    /// Row schema version.
    pub row_version: u32,
    /// Native ED2K profile id.
    pub profile_id: String,
    /// Server bootstrap entries.
    pub servers: Vec<NativeEd2kServerBootstrapEntry>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Native ED2K server bootstrap entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kServerBootstrapEntry {
    /// Server host or DNS name.
    pub host: String,
    /// Server TCP port.
    pub port: u16,
    /// Optional server name.
    pub name: Option<String>,
    /// Optional server description.
    pub description: Option<String>,
    /// Last known user count.
    pub users: Option<u32>,
    /// Last known file count.
    pub files: Option<u32>,
    /// Last known maximum users.
    pub max_users: Option<u32>,
    /// Last known soft file limit.
    pub soft_files: Option<u32>,
    /// Last known hard file limit.
    pub hard_files: Option<u32>,
    /// Last known UDP capability flags.
    pub udp_flags: Option<u32>,
    /// Last known LowID user count.
    pub low_id_users: Option<u32>,
    /// Last known UDP key.
    pub udp_key: Option<u32>,
    /// Optional TCP obfuscation port.
    pub tcp_obfuscation_port: Option<u16>,
    /// Optional UDP obfuscation port.
    pub udp_obfuscation_port: Option<u16>,
}

/// Versioned native ED2K Kad bootstrap persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kKadBootstrapRow {
    /// Row schema version.
    pub row_version: u32,
    /// Native ED2K profile id.
    pub profile_id: String,
    /// Kad bootstrap contacts.
    pub contacts: Vec<NativeEd2kKadBootstrapContact>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Native ED2K Kad bootstrap contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kKadBootstrapContact {
    /// Kad node id.
    pub id: [u8; ED2K_CLIENT_IDENTITY_BYTES],
    /// Contact host.
    pub host: String,
    /// Contact UDP port.
    pub udp_port: u16,
    /// Contact TCP port.
    pub tcp_port: u16,
    /// Kad protocol version.
    pub version: u8,
    /// Whether the endpoint was verified by the source file or bootstrap policy.
    pub verified: bool,
}

/// Versioned native ED2K Kad routing persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kKadRoutingRow {
    /// Row schema version.
    pub row_version: u32,
    /// Native ED2K profile id.
    pub profile_id: String,
    /// Kad routing snapshot serialized by `raria-ed2k`.
    pub routing_snapshot_json: String,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Versioned native ED2K resume persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kResumeRow {
    /// Row schema version.
    pub row_version: u32,
    /// Native task id.
    pub task_id: TaskId,
    /// Total file size in bytes.
    pub file_size: u64,
    /// ED2K root hash.
    pub root_hash: [u8; ED2K_CLIENT_IDENTITY_BYTES],
    /// ED2K part hashes when protocol boundary rules require them.
    pub part_hashes: Vec<[u8; ED2K_CLIENT_IDENTITY_BYTES]>,
    /// Optional AICH root hash.
    pub aich_root: Option<[u8; 20]>,
    /// Ranges verified by disk and integrity truth.
    pub verified_ranges: Vec<ByteRange>,
    /// Ranges queued for re-download after integrity failure.
    pub requeue_ranges: Vec<ByteRange>,
    /// Resumeable ED2K source state.
    pub sources: Vec<NativeEd2kResumeSourceRow>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Native ED2K resume source row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kResumeSourceRow {
    /// Native endpoint string such as `host:port`.
    pub endpoint: String,
    /// Last seen timestamp in caller-owned monotonic or wall-clock seconds.
    pub last_seen_seconds: u64,
    /// Last observed queue rank.
    pub queue_rank: Option<u16>,
}

/// Versioned native ED2K credit persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kCreditRow {
    /// Row schema version.
    pub row_version: u32,
    /// Native ED2K profile id.
    pub profile_id: String,
    /// Remote peer credit entries.
    pub entries: Vec<NativeEd2kCreditEntry>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Native ED2K credit entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kCreditEntry {
    /// Remote ED2K user hash.
    pub user_hash: [u8; ED2K_CLIENT_IDENTITY_BYTES],
    /// Bytes uploaded to this peer.
    pub uploaded_bytes: u64,
    /// Bytes downloaded from this peer.
    pub downloaded_bytes: u64,
}

impl NativeEd2kIdentityRow {
    /// Current native ED2K identity row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native ED2K identity row.
    pub fn new(
        profile_id: impl Into<String>,
        client_hash: [u8; ED2K_CLIENT_IDENTITY_BYTES],
    ) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            profile_id: profile_id.into(),
            client_hash,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedEd2kIdentityRowVersion);
        }
        Ok(())
    }
}

impl NativeEd2kServerBootstrapRow {
    /// Current native ED2K server bootstrap row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native ED2K server bootstrap row.
    pub fn new(
        profile_id: impl Into<String>,
        servers: Vec<NativeEd2kServerBootstrapEntry>,
    ) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            profile_id: profile_id.into(),
            servers,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedEd2kServerBootstrapRowVersion);
        }
        Ok(())
    }
}

impl NativeEd2kKadBootstrapRow {
    /// Current native ED2K Kad bootstrap row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native ED2K Kad bootstrap row.
    pub fn new(
        profile_id: impl Into<String>,
        contacts: Vec<NativeEd2kKadBootstrapContact>,
    ) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            profile_id: profile_id.into(),
            contacts,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedEd2kKadBootstrapRowVersion);
        }
        Ok(())
    }
}

impl NativeEd2kKadRoutingRow {
    /// Current native ED2K Kad routing row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native ED2K Kad routing row.
    pub fn new(profile_id: impl Into<String>, routing_snapshot_json: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            profile_id: profile_id.into(),
            routing_snapshot_json: routing_snapshot_json.into(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedEd2kKadRoutingRowVersion);
        }
        Ok(())
    }
}

impl NativeEd2kResumeRow {
    /// Current native ED2K resume row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native ED2K resume row.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_id: TaskId,
        file_size: u64,
        root_hash: [u8; ED2K_CLIENT_IDENTITY_BYTES],
        part_hashes: Vec<[u8; ED2K_CLIENT_IDENTITY_BYTES]>,
        aich_root: Option<[u8; 20]>,
        verified_ranges: Vec<ByteRange>,
        requeue_ranges: Vec<ByteRange>,
        sources: Vec<NativeEd2kResumeSourceRow>,
    ) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            task_id,
            file_size,
            root_hash,
            part_hashes,
            aich_root,
            verified_ranges,
            requeue_ranges,
            sources,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedEd2kResumeRowVersion);
        }
        for range in self
            .verified_ranges
            .iter()
            .chain(self.requeue_ranges.iter())
        {
            if range.is_empty() || range.end > self.file_size {
                return Err(NativeModelError::InvalidByteRange);
            }
        }
        Ok(())
    }
}

impl NativeEd2kCreditRow {
    /// Current native ED2K credit row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native ED2K credit row.
    pub fn new(profile_id: impl Into<String>, entries: Vec<NativeEd2kCreditEntry>) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            profile_id: profile_id.into(),
            entries,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedEd2kCreditRowVersion);
        }
        Ok(())
    }
}

impl NativeTaskRow {
    /// Current native task row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 2;

    /// Create a native task row.
    pub fn new(task_id: TaskId, lifecycle: TaskLifecycle) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            task_id,
            runtime_bridge_id: None,
            lifecycle,
            job_kind: crate::job::JobKind::Range,
            sources: Vec::new(),
            source_health: std::collections::HashMap::new(),
            output_path: PathBuf::new(),
            total_bytes: None,
            completed_bytes: 0,
            segments: 1,
            created_at: now,
            updated_at: now,
        }
    }

    /// Build a native task row from the current runtime job model.
    pub fn from_runtime_job(job: &crate::job::Job) -> Self {
        let lifecycle = match job.status {
            crate::job::Status::Waiting => TaskLifecycle::Queued,
            crate::job::Status::Active => TaskLifecycle::Running,
            crate::job::Status::Paused => TaskLifecycle::Paused,
            crate::job::Status::Seeding => TaskLifecycle::Seeding,
            crate::job::Status::Complete => TaskLifecycle::Completed,
            crate::job::Status::Error => TaskLifecycle::Failed,
            crate::job::Status::Removed => TaskLifecycle::Removed,
        };
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            task_id: job.task_id.clone(),
            runtime_bridge_id: Some(job.gid.as_raw()),
            lifecycle,
            job_kind: job.kind,
            sources: job.uris.clone(),
            source_health: job.options.source_health.clone(),
            output_path: job.out_path.clone(),
            total_bytes: job.total_size,
            completed_bytes: job.downloaded,
            segments: job.options.max_connections.max(1),
            created_at: job.created_at,
            updated_at: Utc::now(),
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedTaskRowVersion);
        }
        Ok(())
    }

    /// Convert a native task row back into the current runtime job model.
    pub fn to_runtime_job(&self) -> Result<crate::job::Job, NativeModelError> {
        self.validate_version()?;
        let gid = self
            .runtime_bridge_id
            .map(crate::job::Gid::from_raw)
            .ok_or(NativeModelError::MissingRuntimeBridgeId)?;
        let status = match self.lifecycle {
            TaskLifecycle::Queued => crate::job::Status::Waiting,
            TaskLifecycle::Running => crate::job::Status::Waiting,
            TaskLifecycle::Paused => crate::job::Status::Paused,
            TaskLifecycle::Seeding => crate::job::Status::Waiting,
            TaskLifecycle::Completed => crate::job::Status::Complete,
            TaskLifecycle::Failed => crate::job::Status::Error,
            TaskLifecycle::Removed => crate::job::Status::Removed,
        };
        let options = crate::config::JobOptions {
            max_connections: self.segments.max(1),
            source_health: self.source_health.clone(),
            dir: self.output_path.parent().map(PathBuf::from),
            out: self
                .output_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned),
            ..crate::config::JobOptions::default()
        };
        let mut job = match self.job_kind {
            crate::job::JobKind::Range => crate::job::Job::new_range_with_options(
                self.sources.clone(),
                self.output_path.clone(),
                options,
            ),
            crate::job::JobKind::Bt => crate::job::Job::new_bt_with_options(
                self.sources.clone(),
                self.output_path.clone(),
                options,
            ),
            crate::job::JobKind::Ed2k => crate::job::Job::new_ed2k_with_options(
                self.sources.clone(),
                self.output_path.clone(),
                options,
            ),
        };
        job.task_id = self.task_id.clone();
        job.gid = gid;
        job.status = status;
        job.total_size = self.total_bytes;
        job.downloaded = self.completed_bytes;
        job.created_at = self.created_at;
        Ok(job)
    }
}

/// Native projection for an output file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTaskFile {
    /// Stable file identifier scoped to a task.
    pub id: String,
    /// Output path for this file.
    pub path: PathBuf,
    /// Total file length, when known.
    pub length: Option<u64>,
    /// Completed bytes for this file.
    pub completed_bytes: u64,
    /// Whether this file is selected for download.
    pub selected: bool,
}

impl NativeTaskFile {
    /// Create a native file projection.
    pub fn new(id: impl Into<String>, path: PathBuf, length: Option<u64>, selected: bool) -> Self {
        Self {
            id: id.into(),
            path,
            length,
            completed_bytes: 0,
            selected,
        }
    }
}

/// Versioned native segment persistence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSegmentRow {
    /// Row schema version.
    pub row_version: u32,
    /// Stable segment identifier scoped to a task.
    pub id: String,
    /// Related file identifier.
    pub file_id: String,
    /// Assigned source identifier, when selected.
    pub source_id: Option<String>,
    /// Segment byte range.
    pub range: ByteRange,
    /// Completed bytes in this segment.
    pub completed_bytes: u64,
    /// Segment transfer status at the last checkpoint.
    pub status: crate::segment::SegmentStatus,
    /// Entity tag associated with the checkpointed response, when available.
    pub etag: Option<String>,
}

impl NativeSegmentRow {
    /// Current native segment row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native segment row.
    pub fn new(
        id: impl Into<String>,
        file_id: impl Into<String>,
        source_id: Option<impl Into<String>>,
        range: ByteRange,
    ) -> Self {
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            id: id.into(),
            file_id: file_id.into(),
            source_id: source_id.map(Into::into),
            range,
            completed_bytes: 0,
            status: crate::segment::SegmentStatus::Pending,
            etag: None,
        }
    }

    /// Build a persisted native segment row from the current runtime segment state.
    pub fn from_segment_state(id: impl Into<String>, state: &crate::segment::SegmentState) -> Self {
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            id: id.into(),
            file_id: "file_0".to_string(),
            source_id: None,
            range: ByteRange {
                start: state.start,
                end: state.end,
            },
            completed_bytes: state.downloaded,
            status: state.status,
            etag: state.etag.clone(),
        }
    }

    /// Validate that this row can be read by the current binary.
    pub fn validate_version(&self) -> Result<(), NativeModelError> {
        if self.row_version > Self::CURRENT_ROW_VERSION {
            return Err(NativeModelError::UnsupportedSegmentRowVersion);
        }
        Ok(())
    }

    /// Convert the persisted row into the current runtime segment state.
    pub fn to_segment_state(&self) -> Result<crate::segment::SegmentState, NativeModelError> {
        self.validate_version()?;
        Ok(crate::segment::SegmentState {
            start: self.range.start,
            end: self.range.end,
            downloaded: self.completed_bytes,
            etag: self.etag.clone(),
            status: self.status,
        })
    }
}

/// Native piece verification projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTaskPiece {
    /// Stable piece identifier scoped to a task.
    pub id: String,
    /// Related file identifier.
    pub file_id: String,
    /// Piece byte range.
    pub range: ByteRange,
    /// Hash algorithm name.
    pub hash_algorithm: String,
    /// Expected hash encoded as lowercase hex.
    pub expected_hash: String,
    /// Whether this piece has been verified.
    pub verified: bool,
}

impl NativeTaskPiece {
    /// Create a native piece projection.
    pub fn new(
        id: impl Into<String>,
        file_id: impl Into<String>,
        range: ByteRange,
        hash_algorithm: impl Into<String>,
        expected_hash: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            file_id: file_id.into(),
            range,
            hash_algorithm: hash_algorithm.into(),
            expected_hash: expected_hash.into().to_ascii_lowercase(),
            verified: false,
        }
    }

    /// Return a verified projection for externally verified piece state.
    pub fn verified(mut self) -> Self {
        self.verified = true;
        self
    }
}

/// Native summary projection for API and CLI output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTaskSummary {
    /// Public native task identifier.
    pub task_id: TaskId,
    /// Current lifecycle.
    pub lifecycle: TaskLifecycle,
    /// Primary output path for the task.
    pub output_path: PathBuf,
    /// Output files.
    pub files: Vec<NativeTaskFile>,
    /// Sources attached to the task.
    pub sources: Vec<TaskSource>,
    /// Configured segment count.
    pub segments: u32,
    /// Completed payload bytes.
    pub completed_bytes: u64,
    /// Total payload bytes, when known.
    pub total_bytes: Option<u64>,
    /// Current download speed in bytes per second.
    pub download_bytes_per_second: u64,
    /// Active transport connections currently backing the task.
    pub active_connections: u32,
    /// Estimated seconds until completion, when enough runtime data exists.
    pub estimated_seconds_remaining: Option<u64>,
    /// Per-task download limit in bytes per second, or zero for unlimited.
    pub download_bytes_per_second_limit: u64,
    /// Per-task upload limit in bytes per second, or zero for unlimited.
    pub upload_bytes_per_second_limit: u64,
    /// Task creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last projection update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Terminal error message when the task failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// ED2K-specific native status when this is an ED2K task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ed2k: Option<NativeEd2kTaskStatus>,
}

/// Native ED2K task status projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEd2kTaskStatus {
    /// Compact runtime state.
    pub runtime_state: String,
    /// Known source count.
    pub known_sources: u32,
    /// Connected peer count.
    pub connected_peers: u32,
    /// Active upload peer count.
    pub active_upload_peers: u32,
    /// Waiting upload peer count.
    pub waiting_upload_peers: u32,
    /// Whether sharing is enabled for this task.
    pub sharing_enabled: bool,
    /// Whether server discovery is enabled.
    pub server_enabled: bool,
    /// Whether Kad discovery is enabled.
    pub kad_enabled: bool,
}

impl NativeTaskSummary {
    /// Build a native projection from the current runtime job model.
    pub fn from_runtime_job(job: &crate::job::Job) -> Self {
        let task_id = job.task_id.clone();
        let lifecycle = match job.status {
            crate::job::Status::Waiting => TaskLifecycle::Queued,
            crate::job::Status::Active => TaskLifecycle::Running,
            crate::job::Status::Paused => TaskLifecycle::Paused,
            crate::job::Status::Seeding => TaskLifecycle::Seeding,
            crate::job::Status::Complete => TaskLifecycle::Completed,
            crate::job::Status::Error => TaskLifecycle::Failed,
            crate::job::Status::Removed => TaskLifecycle::Removed,
        };
        let files = job
            .bt_files
            .as_ref()
            .map(|files| {
                files
                    .iter()
                    .map(|file| NativeTaskFile {
                        id: format!("file_{}", file.index),
                        path: file.path.clone(),
                        length: Some(file.length),
                        completed_bytes: file.completed_length,
                        selected: file.selected,
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                vec![NativeTaskFile {
                    id: "file_0".to_string(),
                    path: job.out_path.clone(),
                    length: job.total_size,
                    completed_bytes: job.downloaded,
                    selected: true,
                }]
            });
        let sources = job
            .uris
            .iter()
            .filter_map(|uri| {
                TaskSource::new(uri.clone()).ok().map(|source| {
                    let health = job
                        .options
                        .source_health
                        .get(uri)
                        .cloned()
                        .unwrap_or_default();
                    source.with_health(health)
                })
            })
            .collect();

        let estimated_seconds_remaining = match (job.total_size, job.download_speed) {
            (Some(total), speed) if speed > 0 && total > job.downloaded => {
                Some((total - job.downloaded).div_ceil(speed))
            }
            _ => None,
        };
        Self {
            task_id,
            lifecycle,
            output_path: job.out_path.clone(),
            files,
            sources,
            segments: job.options.max_connections.max(1),
            completed_bytes: job.downloaded,
            total_bytes: job.total_size,
            download_bytes_per_second: job.download_speed,
            active_connections: job.connections,
            estimated_seconds_remaining,
            download_bytes_per_second_limit: job.options.max_download_limit,
            upload_bytes_per_second_limit: job.options.max_upload_limit,
            created_at: job.created_at,
            updated_at: Utc::now(),
            error_message: job.error_msg.clone(),
            ed2k: (job.kind == crate::job::JobKind::Ed2k).then(|| NativeEd2kTaskStatus {
                runtime_state: lifecycle.as_str().to_string(),
                known_sources: 0,
                connected_peers: job.connections,
                active_upload_peers: 0,
                waiting_upload_peers: 0,
                sharing_enabled: false,
                server_enabled: true,
                kad_enabled: true,
            }),
        }
    }
}

/// Native BitTorrent peer runtime snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePeerSnapshot {
    /// Stable peer identifier scoped to a task.
    pub id: String,
    /// Peer IP address.
    pub ip: String,
    /// Peer port.
    pub port: u16,
    /// Download speed from this peer in bytes per second.
    pub download_bytes_per_second: u64,
    /// Upload speed to this peer in bytes per second.
    pub upload_bytes_per_second: u64,
    /// Whether this peer reports full payload availability.
    pub seeder: bool,
}

impl NativePeerSnapshot {
    /// Create a native peer snapshot.
    pub fn new(id: impl Into<String>, ip: impl Into<String>, port: u16) -> Self {
        Self {
            id: id.into(),
            ip: ip.into(),
            port,
            download_bytes_per_second: 0,
            upload_bytes_per_second: 0,
            seeder: false,
        }
    }
}

/// Native BitTorrent tracker runtime snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTrackerSnapshot {
    /// Stable tracker identifier scoped to a task.
    pub id: String,
    /// Tracker URI.
    pub uri: String,
    /// Last observed seeder count.
    pub seeders: Option<u32>,
    /// Last observed leecher count.
    pub leechers: Option<u32>,
    /// Last tracker error.
    pub last_error: Option<String>,
    /// Whether the tracker is currently excluded by native policy.
    pub excluded: bool,
    /// Connect timeout in seconds, if configured.
    pub connect_timeout_seconds: Option<u64>,
    /// Announce request timeout in seconds, if configured.
    pub timeout_seconds: Option<u64>,
    /// Announce interval override in seconds, if configured.
    pub interval_seconds: Option<u64>,
}

impl NativeTrackerSnapshot {
    /// Create a native tracker snapshot.
    pub fn new(id: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            uri: uri.into(),
            seeders: None,
            leechers: None,
            last_error: None,
            excluded: false,
            connect_timeout_seconds: None,
            timeout_seconds: None,
            interval_seconds: None,
        }
    }
}

/// Native model validation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NativeModelError {
    /// End offset is lower than the start offset.
    #[error("byte range end must be greater than or equal to start")]
    InvalidByteRange,
    /// Source protocol is not supported by raria.
    #[error("unsupported source protocol")]
    UnsupportedProtocol,
    /// Native task row version is newer than this binary understands.
    #[error("unsupported native task row version")]
    UnsupportedTaskRowVersion,
    /// Native segment persistence row is newer than this binary understands.
    #[error("unsupported native segment row version")]
    UnsupportedSegmentRowVersion,
    /// Native task row lacks the temporary private runtime bridge id.
    #[error("missing runtime bridge id")]
    MissingRuntimeBridgeId,
    /// Native ED2K identity row version is newer than this binary understands.
    #[error("unsupported native ED2K identity row version")]
    UnsupportedEd2kIdentityRowVersion,
    /// Native ED2K server bootstrap row version is newer than this binary understands.
    #[error("unsupported native ED2K server bootstrap row version")]
    UnsupportedEd2kServerBootstrapRowVersion,
    /// Native ED2K Kad bootstrap row version is newer than this binary understands.
    #[error("unsupported native ED2K Kad bootstrap row version")]
    UnsupportedEd2kKadBootstrapRowVersion,
    /// Native ED2K Kad routing row version is newer than this binary understands.
    #[error("unsupported native ED2K Kad routing row version")]
    UnsupportedEd2kKadRoutingRowVersion,
    /// Native ED2K resume row version is newer than this binary understands.
    #[error("unsupported native ED2K resume row version")]
    UnsupportedEd2kResumeRowVersion,
    /// Native ED2K credit row version is newer than this binary understands.
    #[error("unsupported native ED2K credit row version")]
    UnsupportedEd2kCreditRowVersion,
    /// Native task id is malformed.
    #[error("invalid native task id")]
    InvalidTaskId,
}

fn source_id(uri: &str) -> String {
    let digest = Sha256::digest(uri.as_bytes());
    format!("src_{}", hex::encode(&digest[..8]))
}
