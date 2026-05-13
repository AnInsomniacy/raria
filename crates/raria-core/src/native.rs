//! Native raria task and event model.

use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

    /// Build the temporary migration identifier for an existing numeric job ID.
    pub fn from_migration_gid(raw: u64) -> Self {
        Self(format!("task_migration_{raw:016x}"))
    }

    /// Parse a task identifier received from a native public surface.
    pub fn parse(value: impl Into<String>) -> Result<Self, NativeModelError> {
        let value = value.into();
        if value.starts_with("task_") {
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

/// In-memory mapping from native task identifiers to the current runtime job id.
#[derive(Debug, Clone, Default)]
pub struct NativeTaskIndex {
    by_task_id: HashMap<TaskId, crate::job::Gid>,
    by_gid: HashMap<crate::job::Gid, TaskId>,
}

impl NativeTaskIndex {
    /// Register a runtime job under a native task id.
    pub fn register(&mut self, task_id: TaskId, gid: crate::job::Gid) {
        self.by_gid.insert(gid, task_id.clone());
        self.by_task_id.insert(task_id, gid);
    }

    /// Register a migration job using its deterministic temporary task id.
    pub fn register_migration_gid(&mut self, gid: crate::job::Gid) -> TaskId {
        let task_id = TaskId::from_migration_gid(gid.as_raw());
        self.register(task_id.clone(), gid);
        task_id
    }

    /// Resolve a native task id to the current runtime job id.
    pub fn gid_for_task_id(&self, task_id: &TaskId) -> Option<crate::job::Gid> {
        self.by_task_id.get(task_id).copied()
    }

    /// Resolve a runtime job id to the native task id.
    pub fn task_id_for_gid(&self, gid: crate::job::Gid) -> Option<TaskId> {
        self.by_gid.get(&gid).cloned()
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
        if uri.starts_with("metalink:") {
            return Ok(Self::Metalink);
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
        })
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
    /// Last successful migration timestamp.
    pub last_migrated_at: Option<DateTime<Utc>>,
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
            last_migrated_at: None,
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
    /// Source URIs assigned to the task.
    pub sources: Vec<String>,
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

impl NativeTaskRow {
    /// Current native task row schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;

    /// Create a native task row.
    pub fn new(task_id: TaskId, lifecycle: TaskLifecycle) -> Self {
        let now = Utc::now();
        Self {
            row_version: Self::CURRENT_ROW_VERSION,
            task_id,
            runtime_bridge_id: None,
            lifecycle,
            sources: Vec::new(),
            output_path: PathBuf::new(),
            total_bytes: None,
            completed_bytes: 0,
            segments: 1,
            created_at: now,
            updated_at: now,
        }
    }

    /// Build a native task row from the current job model during migration.
    pub fn from_job_for_migration(job: &crate::job::Job) -> Self {
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
            sources: job.uris.clone(),
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

    /// Convert a migration task row back into the current job model.
    pub fn to_job_for_migration(&self) -> Result<crate::job::Job, NativeModelError> {
        self.validate_version()?;
        let gid = if let Some(raw) = self.runtime_bridge_id {
            crate::job::Gid::from_raw(raw)
        } else {
            let Some(raw) = self.task_id.as_str().strip_prefix("task_migration_") else {
                return Err(NativeModelError::UnsupportedTaskIdForMigration);
            };
            u64::from_str_radix(raw, 16)
                .map(crate::job::Gid::from_raw)
                .map_err(|_| NativeModelError::UnsupportedTaskIdForMigration)?
        };
        let status = match self.lifecycle {
            TaskLifecycle::Queued => crate::job::Status::Waiting,
            TaskLifecycle::Running => crate::job::Status::Waiting,
            TaskLifecycle::Paused => crate::job::Status::Paused,
            TaskLifecycle::Seeding => crate::job::Status::Waiting,
            TaskLifecycle::Completed => crate::job::Status::Complete,
            TaskLifecycle::Failed => crate::job::Status::Error,
            TaskLifecycle::Removed => crate::job::Status::Removed,
        };
        let mut job = crate::job::Job::new_range_with_options(
            self.sources.clone(),
            self.output_path.clone(),
            crate::config::JobOptions {
                max_connections: self.segments.max(1),
                dir: self.output_path.parent().map(PathBuf::from),
                out: self
                    .output_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned),
                ..crate::config::JobOptions::default()
            },
        );
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
        }
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
            expected_hash: expected_hash.into(),
            verified: false,
        }
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
    /// Output files.
    pub files: Vec<NativeTaskFile>,
    /// Sources attached to the task.
    pub sources: Vec<TaskSource>,
    /// Completed payload bytes.
    pub completed_bytes: u64,
    /// Total payload bytes, when known.
    pub total_bytes: Option<u64>,
    /// Current download speed in bytes per second.
    pub download_bytes_per_second: u64,
}

impl NativeTaskSummary {
    /// Build a native projection from the current job model during migration.
    pub fn from_job_for_migration(job: &crate::job::Job) -> Self {
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
        let files = vec![NativeTaskFile {
            id: "file_0".to_string(),
            path: job.out_path.clone(),
            length: job.total_size,
            completed_bytes: job.downloaded,
            selected: true,
        }];
        let sources = job
            .uris
            .iter()
            .filter_map(|uri| TaskSource::new(uri.clone()).ok())
            .collect();

        Self {
            task_id,
            lifecycle,
            files,
            sources,
            completed_bytes: job.downloaded,
            total_bytes: job.total_size,
            download_bytes_per_second: job.download_speed,
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
    /// Native task id cannot be mapped into the migration job model.
    #[error("unsupported native task id for migration")]
    UnsupportedTaskIdForMigration,
    /// Native task id is malformed.
    #[error("invalid native task id")]
    InvalidTaskId,
}

fn source_id(uri: &str) -> String {
    let digest = Sha256::digest(uri.as_bytes());
    format!("src_{}", hex::encode(&digest[..8]))
}
