#![warn(missing_docs)]
//! # raria-core
//!
//! Core engine for the raria multi-protocol download manager.
//!
//! This crate provides the foundational types and orchestration logic:
//! - [`job::Job`] — download task model with state machine
//!   (Waiting → Active → Complete / Error / Paused / Removed)
//! - [`engine::Engine`] — central coordinator for job lifecycle
//! - [`scheduler::Scheduler`] — FIFO queue with configurable concurrency
//! - [`registry::JobRegistry`] — thread-safe in-memory job index
//! - [`cancel::CancelRegistry`] — per-job cancellation token management
//! - [`persist::Store`] — crash-safe persistence layer via `redb`
//! - [`progress::EventBus`] — `tokio::broadcast` channel for download events
//! - [`limiter`] — shared rate limiter using `governor` + `arc-swap`
//!
//! ## Architecture
//!
//! ```text
//! Engine ──┬── JobRegistry  (in-memory job index)
//!          ├── Scheduler    (waiting queue + concurrency limit)
//!          ├── CancelRegistry (per-job CancellationToken)
//!          ├── Store        (redb atomic persistence)
//!          └── EventBus     (progress / status broadcast)
//! ```
//!
//! Protocol-specific backends (HTTP, FTP, SFTP, BT) live in separate crates
//! and depend on `raria-core` for job model and engine types.

/// Per-job cancellation token management.
pub mod cancel;
/// Checksum verification (SHA-256, SHA-1, MD5).
pub mod checksum;
/// Global configuration struct and defaults.
pub mod config;
/// Central download engine — job lifecycle coordinator.
pub mod engine;
/// Pre-allocation strategies for download files (fallocate / trunc / none).
pub mod file_alloc;
/// Input file parser (newline-delimited URI lists).
pub mod input_file;
/// Download job model: GID, status machine, options, metadata.
pub mod job;
/// Global and per-job rate limiting.
pub mod limiter;
/// Shared structured lifecycle logging helpers.
pub mod logging;
/// Native raria task, source, file, segment, piece, and event model.
pub mod native;
/// Native `raria.toml` configuration schema and loader.
pub mod native_config;
/// Crash-safe persistence via redb (job state + segment checkpoints).
pub mod persist;
/// Broadcast event bus for download progress and status changes.
pub mod progress;
/// Thread-safe in-memory job index.
pub mod registry;
/// Output file rename strategies (append counter, overwrite, skip).
pub mod rename;
/// FIFO waiting-queue with configurable concurrency limit.
pub mod scheduler;
/// Byte-range segment planning and state tracking.
pub mod segment;
/// Protocol-agnostic download service trait.
pub mod service;
/// Per-job speed measurement over sliding windows.
pub mod speed;

#[cfg(test)]
mod native_model_tests {
    use crate::native::{
        ByteRange, NativeEvent, NativeEventData, NativeEventType, SourceProtocol, TaskId,
        TaskLifecycle, TaskSource,
    };

    #[test]
    fn task_id_is_opaque_and_not_runtime_hex() {
        let id = TaskId::new();
        let rendered = id.as_str();

        assert!(rendered.starts_with("task_"));
        assert_eq!(rendered.len(), "task_".len() + 32);
        assert!(
            rendered["task_".len()..]
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
        );
    }

    #[test]
    fn lifecycle_names_are_native() {
        assert_eq!(TaskLifecycle::Queued.as_str(), "queued");
        assert_eq!(TaskLifecycle::Running.as_str(), "running");
        assert_eq!(TaskLifecycle::Seeding.as_str(), "seeding");
        assert_eq!(TaskLifecycle::Completed.as_str(), "completed");
    }

    #[test]
    fn byte_range_uses_exclusive_end_offsets() {
        let range = ByteRange::new(100, 260).expect("valid byte range");

        assert_eq!(range.start, 100);
        assert_eq!(range.end, 260);
        assert_eq!(range.len(), 160);
    }

    #[test]
    fn byte_range_rejects_reversed_offsets() {
        let err = ByteRange::new(260, 100).expect_err("reversed range must fail");

        assert_eq!(
            err.to_string(),
            "byte range end must be greater than or equal to start"
        );
    }

    #[test]
    fn task_source_detects_common_protocols() {
        let source = TaskSource::new("https://example.com/file.iso").expect("valid source");
        assert_eq!(source.protocol, SourceProtocol::Https);

        let source = TaskSource::new("magnet:?xt=urn:btih:abcdef").expect("valid source");
        assert_eq!(source.protocol, SourceProtocol::Magnet);

        let source = TaskSource::new("/downloads/linux.iso.torrent").expect("valid source");
        assert_eq!(source.protocol, SourceProtocol::Torrent);
    }

    #[test]
    fn native_event_uses_versioned_event_envelope() {
        let task_id = TaskId::new();
        let event = NativeEvent::new(
            42,
            NativeEventType::TaskProgress,
            Some(task_id.clone()),
            NativeEventData::Progress {
                completed_bytes: 1024,
                total_bytes: Some(2048),
                download_bytes_per_second: 512,
            },
        );

        assert_eq!(event.version, 1);
        assert_eq!(event.sequence, 42);
        assert_eq!(event.event_type.as_str(), "task.progress");
        assert_eq!(event.task_id, Some(task_id));
    }
}

#[cfg(test)]
mod native_persist_tests {
    use crate::native::{
        NativeSegmentRow, NativeStoreMetadata, NativeTaskRow, TaskId, TaskLifecycle,
    };

    #[test]
    fn store_metadata_has_explicit_schema_version() {
        let metadata = NativeStoreMetadata::new("store-test");

        assert_eq!(
            metadata.schema_version,
            NativeStoreMetadata::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(metadata.store_id, "store-test");
        assert!(metadata.last_migrated_at.is_none());
    }

    #[test]
    fn task_rows_are_versioned_independently_from_runtime_structs() {
        let task_id = TaskId::new();
        let row = NativeTaskRow::new(task_id.clone(), TaskLifecycle::Queued);

        assert_eq!(row.row_version, NativeTaskRow::CURRENT_ROW_VERSION);
        assert_eq!(row.task_id, task_id);
        assert_eq!(row.lifecycle, TaskLifecycle::Queued);
    }

    #[test]
    fn task_row_rejects_unknown_future_versions() {
        let mut row = NativeTaskRow::new(TaskId::new(), TaskLifecycle::Queued);
        row.row_version = NativeTaskRow::CURRENT_ROW_VERSION + 1;

        let err = row
            .validate_version()
            .expect_err("future row version must fail");

        assert_eq!(err.to_string(), "unsupported native task row version");
    }

    #[test]
    fn task_row_carries_runtime_job_restore_fields() {
        let mut job = crate::job::Job::new_range_with_options(
            vec!["https://example.com/file.iso".into()],
            std::path::PathBuf::from("/tmp/file.iso"),
            crate::config::JobOptions {
                max_connections: 8,
                ..crate::config::JobOptions::default()
            },
        );
        job.total_size = Some(1024);
        job.downloaded = 256;
        job.options.source_health.insert(
            "https://example.com/file.iso".to_string(),
            crate::native::NativeSourceHealth::healthy(4096),
        );

        let row = NativeTaskRow::from_runtime_job(&job);

        assert_eq!(row.task_id, job.task_id);
        assert_eq!(row.runtime_bridge_id, Some(job.gid.as_raw()));
        assert_eq!(row.lifecycle, TaskLifecycle::Queued);
        assert_eq!(row.sources, vec!["https://example.com/file.iso"]);
        assert_eq!(row.output_path, std::path::PathBuf::from("/tmp/file.iso"));
        assert_eq!(row.total_bytes, Some(1024));
        assert_eq!(row.completed_bytes, 256);
        assert_eq!(row.segments, 8);
        assert_eq!(
            row.source_health["https://example.com/file.iso"].last_download_bytes_per_second,
            Some(4096)
        );
    }

    #[test]
    fn task_row_restores_runtime_job_fields() {
        let mut job = crate::job::Job::new_range_with_options(
            vec!["https://example.com/file.iso".into()],
            std::path::PathBuf::from("/tmp/file.iso"),
            crate::config::JobOptions {
                max_connections: 8,
                ..crate::config::JobOptions::default()
            },
        );
        job.total_size = Some(1024);
        job.downloaded = 256;
        job.options.source_health.insert(
            "https://example.com/file.iso".to_string(),
            crate::native::NativeSourceHealth::failed(1, "transient error: timeout"),
        );
        let row = NativeTaskRow::from_runtime_job(&job);

        let restored = row.to_runtime_job().expect("restored job");

        assert_eq!(restored.gid, job.gid);
        assert_eq!(restored.status, crate::job::Status::Waiting);
        assert_eq!(restored.uris, job.uris);
        assert_eq!(restored.out_path, job.out_path);
        assert_eq!(restored.total_size, Some(1024));
        assert_eq!(restored.downloaded, 256);
        assert_eq!(restored.options.max_connections, 8);
        assert_eq!(
            restored.options.source_health["https://example.com/file.iso"]
                .last_error
                .as_deref(),
            Some("transient error: timeout")
        );
    }

    #[test]
    fn task_row_restores_opaque_task_id_using_runtime_bridge() {
        let mut job = crate::job::Job::new_range(
            vec!["https://example.com/file.iso".into()],
            std::path::PathBuf::from("/tmp/file.iso"),
        );
        job.gid = crate::job::Gid::from_raw(99);
        let mut row = NativeTaskRow::from_runtime_job(&job);
        row.task_id = TaskId::new();

        let restored = row.to_runtime_job().expect("restored job");

        assert_eq!(restored.gid, crate::job::Gid::from_raw(99));
    }

    #[test]
    fn task_row_requires_explicit_runtime_bridge_for_job_restore() {
        let mut row = NativeTaskRow::new(TaskId::new(), TaskLifecycle::Queued);
        row.sources = vec!["https://example.com/file.iso".into()];
        row.output_path = std::path::PathBuf::from("/tmp/file.iso");

        let err = row
            .to_runtime_job()
            .expect_err("task id fallback must not restore runtime job id");

        assert_eq!(err.to_string(), "missing runtime bridge id");
    }

    #[test]
    fn segment_rows_are_versioned_independently_from_runtime_structs() {
        let state = crate::segment::SegmentState {
            start: 100,
            end: 300,
            downloaded: 75,
            etag: Some("etag-v1".to_string()),
            status: crate::segment::SegmentStatus::Active,
        };
        let row = NativeSegmentRow::from_segment_state("segment_0", &state);

        assert_eq!(row.row_version, NativeSegmentRow::CURRENT_ROW_VERSION);
        assert_eq!(row.range.start, 100);
        assert_eq!(row.range.end, 300);
        assert_eq!(row.completed_bytes, 75);
        assert_eq!(row.etag.as_deref(), Some("etag-v1"));
    }

    #[test]
    fn segment_row_rejects_unknown_future_versions() {
        let mut row = NativeSegmentRow::new(
            "segment_0",
            "file_0",
            None::<String>,
            crate::native::ByteRange::new(0, 100).expect("range"),
        );
        row.row_version = NativeSegmentRow::CURRENT_ROW_VERSION + 1;

        let err = row
            .validate_version()
            .expect_err("future row version must fail");

        assert_eq!(err.to_string(), "unsupported native segment row version");
    }

    #[test]
    fn segment_row_restores_runtime_segment_state() {
        let state = crate::segment::SegmentState {
            start: 100,
            end: 300,
            downloaded: 75,
            etag: Some("etag-v1".to_string()),
            status: crate::segment::SegmentStatus::Active,
        };
        let row = NativeSegmentRow::from_segment_state("segment_0", &state);

        let restored = row.to_segment_state().expect("segment state");

        assert_eq!(restored.start, state.start);
        assert_eq!(restored.end, state.end);
        assert_eq!(restored.downloaded, state.downloaded);
        assert_eq!(restored.etag, state.etag);
        assert_eq!(restored.status, state.status);
    }
}

#[cfg(test)]
mod native_projection_tests {
    use crate::job::{BtFile, Gid, Job, Status};
    use crate::native::{
        ByteRange, NativeEvent, NativeEventData, NativeEventType, NativePeerSnapshot,
        NativeSegmentRow, NativeTaskFile, NativeTaskPiece, NativeTaskSummary,
        NativeTrackerSnapshot, SourceProtocol, TaskId, TaskLifecycle,
    };
    use std::path::PathBuf;

    #[test]
    fn native_task_file_tracks_selection_and_progress() {
        let file = NativeTaskFile::new("file_1", PathBuf::from("image.iso"), Some(4096), true);

        assert_eq!(file.id, "file_1");
        assert_eq!(file.length, Some(4096));
        assert!(file.selected);
        assert_eq!(file.completed_bytes, 0);
    }

    #[test]
    fn native_segment_row_tracks_file_source_and_checkpoint_state() {
        let range = ByteRange::new(0, 1024).expect("valid range");
        let segment = NativeSegmentRow::new("seg_1", "file_1", Some("src_1"), range);

        assert_eq!(segment.row_version, NativeSegmentRow::CURRENT_ROW_VERSION);
        assert_eq!(segment.file_id, "file_1");
        assert_eq!(segment.source_id.as_deref(), Some("src_1"));
        assert_eq!(segment.range.len(), 1024);
    }

    #[test]
    fn native_piece_tracks_expected_hash() {
        let range = ByteRange::new(0, 16384).expect("valid range");
        let piece = NativeTaskPiece::new("piece_1", "file_1", range, "sha-256", "ABC123");

        assert_eq!(piece.id, "piece_1");
        assert_eq!(piece.hash_algorithm, "sha-256");
        assert_eq!(piece.expected_hash, "abc123");
        assert!(!piece.verified);
        assert!(piece.verified().verified);
    }

    #[test]
    fn task_summary_projection_from_runtime_job_is_native_adapter() {
        let mut job = Job::new_range(
            vec!["https://example.com/file.iso".into()],
            PathBuf::from("/tmp/file.iso"),
        );
        job.gid = Gid::from_raw(7);
        job.status = Status::Active;
        job.total_size = Some(2048);
        job.downloaded = 512;
        job.download_speed = 128;

        let summary = NativeTaskSummary::from_runtime_job(&job);

        assert_eq!(summary.lifecycle, TaskLifecycle::Running);
        assert_eq!(summary.completed_bytes, 512);
        assert_eq!(summary.total_bytes, Some(2048));
        assert_eq!(summary.sources[0].protocol, SourceProtocol::Https);
    }

    #[test]
    fn task_summary_projection_uses_bt_file_snapshots() {
        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:feedface".into()],
            PathBuf::from("/tmp/torrent"),
        );
        job.bt_files = Some(vec![
            BtFile {
                index: 0,
                path: PathBuf::from("disc/file-a.bin"),
                length: 100,
                completed_length: 25,
                selected: true,
            },
            BtFile {
                index: 1,
                path: PathBuf::from("disc/file-b.bin"),
                length: 200,
                completed_length: 0,
                selected: false,
            },
        ]);

        let summary = NativeTaskSummary::from_runtime_job(&job);

        assert_eq!(summary.files.len(), 2);
        assert_eq!(summary.files[0].id, "file_0");
        assert_eq!(summary.files[0].path, PathBuf::from("disc/file-a.bin"));
        assert_eq!(summary.files[0].completed_bytes, 25);
        assert!(summary.files[0].selected);
        assert_eq!(summary.files[1].id, "file_1");
        assert!(!summary.files[1].selected);
    }

    #[test]
    fn task_id_parse_accepts_native_opaque_ids_only() {
        let task_id = TaskId::parse("task_0123456789abcdef0123456789abcdef").expect("task id");

        assert_eq!(task_id.as_str(), "task_0123456789abcdef0123456789abcdef");
        assert!(TaskId::parse("gid_123").is_err());
        assert!(TaskId::parse("task_custom").is_err());
    }

    #[test]
    fn peer_snapshot_exposes_native_peer_state() {
        let peer = NativePeerSnapshot::new("peer_1", "203.0.113.7", 6881);

        assert_eq!(peer.id, "peer_1");
        assert_eq!(peer.ip, "203.0.113.7");
        assert_eq!(peer.port, 6881);
        assert_eq!(peer.download_bytes_per_second, 0);
    }

    #[test]
    fn tracker_snapshot_exposes_native_tracker_state() {
        let tracker = NativeTrackerSnapshot::new("tracker_1", "udp://tracker.example:6969");

        assert_eq!(tracker.id, "tracker_1");
        assert_eq!(tracker.uri, "udp://tracker.example:6969");
        assert_eq!(tracker.seeders, None);
        assert_eq!(tracker.leechers, None);
        assert!(!tracker.excluded);
        assert_eq!(tracker.connect_timeout_seconds, None);
        assert_eq!(tracker.timeout_seconds, None);
        assert_eq!(tracker.interval_seconds, None);
    }

    #[test]
    fn bt_native_events_use_stable_type_strings_and_payloads() {
        let task_id = TaskId::new();
        let event = NativeEvent::new(
            1,
            NativeEventType::TaskBtMetadataResolved,
            Some(task_id.clone()),
            NativeEventData::BtMetadata {
                info_hash: "0123456789abcdef".to_string(),
                name: Some("fixture.iso".to_string()),
                total_bytes: Some(4096),
                piece_length: Some(1024),
                piece_count: Some(4),
            },
        );

        let json = serde_json::to_value(event).expect("event json");

        assert_eq!(json["type"], "task.bt.metadata.resolved");
        assert_eq!(json["taskId"], task_id.as_str());
        assert_eq!(json["data"]["kind"], "btMetadata");
        assert_eq!(json["data"]["infoHash"], "0123456789abcdef");

        assert_eq!(
            NativeEventType::TaskBtSeedingStarted.as_str(),
            "task.bt.seeding.started"
        );
        assert_eq!(
            NativeEventType::TaskBtPeerUpdated.as_str(),
            "task.bt.peer.updated"
        );
        assert_eq!(
            NativeEventType::TaskBtTrackerUpdated.as_str(),
            "task.bt.tracker.updated"
        );
    }
}
