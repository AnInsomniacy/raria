// raria-core: Job types, GID management, and status tracking.
//
// This module defines the core data model for download tasks in raria.
// Every download—whether HTTP, FTP, SFTP, or BitTorrent—is represented
// as a `Job` with a unique `Gid` and tracked through a `Status` state machine.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global ID counter for generating unique GIDs.
static NEXT_GID: AtomicU64 = AtomicU64::new(1);

/// A globally unique identifier for a download job.
///
/// GIDs are 64-bit integers rendered as zero-padded 16-character hex strings
/// to maintain compatibility with aria2's GID format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Gid(u64);

impl Gid {
    /// Allocate a new unique GID.
    pub fn new() -> Self {
        Self(NEXT_GID.fetch_add(1, Ordering::Relaxed))
    }

    /// Construct a GID from a raw u64 value.
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Return the raw u64 value.
    pub fn as_raw(&self) -> u64 {
        self.0
    }

    /// Reset the global counter (for testing only).
    #[cfg(test)]
    pub fn reset_counter() {
        NEXT_GID.store(1, Ordering::Relaxed);
    }
}

impl Default for Gid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Gid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// The lifecycle status of a download job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Actively downloading.
    Active,
    /// Queued and waiting for a slot.
    Waiting,
    /// Paused by the user.
    Paused,
    /// Download completed successfully.
    Complete,
    /// Download failed with an error.
    Error,
    /// Removed by the user.
    Removed,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Waiting => write!(f, "waiting"),
            Self::Paused => write!(f, "paused"),
            Self::Complete => write!(f, "complete"),
            Self::Error => write!(f, "error"),
            Self::Removed => write!(f, "removed"),
        }
    }
}

/// Discriminator for the type of download backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    /// Byte-range protocols: HTTP, FTP, SFTP.
    Range,
    /// BitTorrent (managed by librqbit).
    Bt,
}

/// A download job with all metadata needed for scheduling and persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique identifier.
    pub gid: Gid,
    /// The kind of download backend.
    pub kind: JobKind,
    /// Current lifecycle status.
    pub status: Status,
    /// Source URIs (multiple for multi-source / fallback).
    pub uris: Vec<String>,
    /// Output file path.
    pub out_path: PathBuf,
    /// Total file size in bytes, if known.
    pub total_size: Option<u64>,
    /// Bytes downloaded so far.
    pub downloaded: u64,
    /// Upload speed (BT only), bytes/sec.
    pub upload_speed: u64,
    /// Download speed, bytes/sec.
    pub download_speed: u64,
    /// Timestamp when the job was created.
    pub created_at: DateTime<Utc>,
    /// Error message if `status == Error`.
    pub error_msg: Option<String>,
}

impl Job {
    /// Create a new Range (HTTP/FTP/SFTP) job.
    pub fn new_range(uris: Vec<String>, out_path: PathBuf) -> Self {
        Self {
            gid: Gid::new(),
            kind: JobKind::Range,
            status: Status::Waiting,
            uris,
            out_path,
            total_size: None,
            downloaded: 0,
            upload_speed: 0,
            download_speed: 0,
            created_at: Utc::now(),
            error_msg: None,
        }
    }

    /// Create a new BitTorrent job.
    pub fn new_bt(uris: Vec<String>, out_path: PathBuf) -> Self {
        Self {
            gid: Gid::new(),
            kind: JobKind::Bt,
            status: Status::Waiting,
            uris,
            out_path,
            total_size: None,
            downloaded: 0,
            upload_speed: 0,
            download_speed: 0,
            created_at: Utc::now(),
            error_msg: None,
        }
    }

    /// Transition to a new status, returning an error if the transition is invalid.
    pub fn transition(&mut self, new_status: Status) -> Result<(), InvalidTransition> {
        if self.is_valid_transition(new_status) {
            self.status = new_status;
            Ok(())
        } else {
            Err(InvalidTransition {
                from: self.status,
                to: new_status,
            })
        }
    }

    /// Check if a status transition is valid according to the state machine.
    ///
    /// Valid transitions:
    /// ```text
    /// Waiting  → Active | Paused | Removed
    /// Active   → Paused | Complete | Error | Removed
    /// Paused   → Waiting | Removed
    /// Error    → Waiting | Removed
    /// Complete → Removed
    /// Removed  → (terminal)
    /// ```
    pub fn is_valid_transition(&self, new_status: Status) -> bool {
        matches!(
            (self.status, new_status),
            (Status::Waiting, Status::Active)
                | (Status::Waiting, Status::Paused)
                | (Status::Waiting, Status::Removed)
                | (Status::Active, Status::Paused)
                | (Status::Active, Status::Complete)
                | (Status::Active, Status::Error)
                | (Status::Active, Status::Removed)
                | (Status::Paused, Status::Waiting)
                | (Status::Paused, Status::Removed)
                | (Status::Error, Status::Waiting)
                | (Status::Error, Status::Removed)
                | (Status::Complete, Status::Removed)
        )
    }

    /// Calculate completion percentage (0.0 to 100.0).
    pub fn progress_pct(&self) -> f64 {
        match self.total_size {
            Some(total) if total > 0 => (self.downloaded as f64 / total as f64) * 100.0,
            _ => 0.0,
        }
    }
}

/// Error for invalid status transitions.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid transition from {from} to {to}")]
pub struct InvalidTransition {
    pub from: Status,
    pub to: Status,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gid_generates_unique_values() {
        Gid::reset_counter();
        let g1 = Gid::new();
        let g2 = Gid::new();
        assert_ne!(g1, g2);
        assert_eq!(g1.as_raw() + 1, g2.as_raw());
    }

    #[test]
    fn gid_display_is_16_char_hex() {
        let gid = Gid::from_raw(255);
        let display = format!("{}", gid);
        assert_eq!(display.len(), 16);
        assert_eq!(display, "00000000000000ff");
    }

    #[test]
    fn gid_from_raw_roundtrips() {
        let gid = Gid::from_raw(42);
        assert_eq!(gid.as_raw(), 42);
    }

    #[test]
    fn gid_serde_roundtrips() {
        let gid = Gid::from_raw(999);
        let json = serde_json::to_string(&gid).unwrap();
        let recovered: Gid = serde_json::from_str(&json).unwrap();
        assert_eq!(gid, recovered);
    }

    #[test]
    fn status_display_matches_serde_name() {
        assert_eq!(format!("{}", Status::Active), "active");
        assert_eq!(format!("{}", Status::Waiting), "waiting");
        assert_eq!(format!("{}", Status::Paused), "paused");
        assert_eq!(format!("{}", Status::Complete), "complete");
        assert_eq!(format!("{}", Status::Error), "error");
        assert_eq!(format!("{}", Status::Removed), "removed");
    }

    #[test]
    fn status_serde_roundtrips() {
        let json = serde_json::to_string(&Status::Active).unwrap();
        assert_eq!(json, "\"active\"");
        let recovered: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, Status::Active);
    }

    #[test]
    fn new_range_job_starts_waiting() {
        Gid::reset_counter();
        let job = Job::new_range(
            vec!["https://example.com/file.zip".into()],
            PathBuf::from("/tmp/file.zip"),
        );
        assert_eq!(job.status, Status::Waiting);
        assert_eq!(job.kind, JobKind::Range);
        assert_eq!(job.downloaded, 0);
        assert!(job.total_size.is_none());
        assert!(job.error_msg.is_none());
    }

    #[test]
    fn new_bt_job_starts_waiting() {
        let job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:abc123".into()],
            PathBuf::from("/tmp/downloads"),
        );
        assert_eq!(job.status, Status::Waiting);
        assert_eq!(job.kind, JobKind::Bt);
    }

    #[test]
    fn valid_transitions_succeed() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));

        // Waiting → Active
        assert!(job.transition(Status::Active).is_ok());
        assert_eq!(job.status, Status::Active);

        // Active → Paused
        assert!(job.transition(Status::Paused).is_ok());
        assert_eq!(job.status, Status::Paused);

        // Paused → Waiting
        assert!(job.transition(Status::Waiting).is_ok());
        assert_eq!(job.status, Status::Waiting);

        // Waiting → Active → Complete
        assert!(job.transition(Status::Active).is_ok());
        assert!(job.transition(Status::Complete).is_ok());
        assert_eq!(job.status, Status::Complete);

        // Complete → Removed
        assert!(job.transition(Status::Removed).is_ok());
        assert_eq!(job.status, Status::Removed);
    }

    #[test]
    fn invalid_transitions_fail() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));

        // Waiting → Complete is invalid
        let err = job.transition(Status::Complete).unwrap_err();
        assert_eq!(err.from, Status::Waiting);
        assert_eq!(err.to, Status::Complete);

        // Status should not have changed
        assert_eq!(job.status, Status::Waiting);
    }

    #[test]
    fn removed_is_terminal() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.transition(Status::Active).unwrap();
        job.transition(Status::Removed).unwrap();

        // Cannot transition out of Removed
        assert!(job.transition(Status::Waiting).is_err());
        assert!(job.transition(Status::Active).is_err());
        assert!(job.transition(Status::Paused).is_err());
    }

    #[test]
    fn error_can_retry() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.transition(Status::Active).unwrap();
        job.transition(Status::Error).unwrap();

        // Error → Waiting (retry)
        assert!(job.transition(Status::Waiting).is_ok());
    }

    #[test]
    fn progress_pct_correct() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.total_size = Some(1000);
        job.downloaded = 250;
        let pct = job.progress_pct();
        assert!((pct - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_pct_unknown_size_is_zero() {
        let job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        assert!((job.progress_pct() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_pct_zero_total_is_zero() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.total_size = Some(0);
        assert!((job.progress_pct() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_pct_complete() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.total_size = Some(500);
        job.downloaded = 500;
        assert!((job.progress_pct() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn job_serde_roundtrips() {
        let job = Job::new_range(
            vec!["https://example.com/file.zip".into()],
            PathBuf::from("/tmp/file.zip"),
        );
        let json = serde_json::to_string(&job).unwrap();
        let recovered: Job = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.gid, job.gid);
        assert_eq!(recovered.kind, job.kind);
        assert_eq!(recovered.status, job.status);
        assert_eq!(recovered.uris, job.uris);
    }
}
