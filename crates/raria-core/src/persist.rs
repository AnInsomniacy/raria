// raria-core: Persistence layer using redb.
//
// This module implements the storage backend for job metadata, segment state,
// job options, and global state using redb — an embedded key-value store.

use crate::config::JobOptions;
use crate::job::{Gid, Job};
use crate::native::{NativeStoreMetadata, NativeTaskRow, TaskId};
use crate::segment::SegmentState;
use anyhow::{Context, Result};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;

/// Table: jobs — stores serialized Job structs keyed by raw GID.
const JOBS_TABLE: TableDefinition<u64, &str> = TableDefinition::new("jobs");

/// Table: segments — stores serialized SegmentState keyed by (gid, segment_id).
const SEGMENTS_TABLE: TableDefinition<(u64, u32), &str> = TableDefinition::new("segments");

/// Table: job_options — stores serialized JobOptions keyed by raw GID.
const JOB_OPTIONS_TABLE: TableDefinition<u64, &str> = TableDefinition::new("job_options");

/// Table: global_state — stores global key-value pairs (e.g., "next_gid", "config").
const GLOBAL_STATE_TABLE: TableDefinition<&str, &str> = TableDefinition::new("global_state");

/// Table: native_metadata — stores versioned native store metadata.
const NATIVE_METADATA_TABLE: TableDefinition<&str, &str> = TableDefinition::new("native_metadata");

/// Table: native_tasks — stores versioned native task rows keyed by task id.
const NATIVE_TASKS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("native_tasks");

/// Table: native_segments — stores SegmentState keyed by native task id and segment id.
const NATIVE_SEGMENTS_TABLE: TableDefinition<(&str, u32), &str> =
    TableDefinition::new("native_segments");

/// Persistent storage for raria state.
#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
}

impl Store {
    /// Open or create a store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path).context("failed to open redb database")?;

        // Ensure all tables exist.
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(JOBS_TABLE)?;
            let _ = write_txn.open_table(SEGMENTS_TABLE)?;
            let _ = write_txn.open_table(JOB_OPTIONS_TABLE)?;
            let _ = write_txn.open_table(GLOBAL_STATE_TABLE)?;
            let mut metadata = write_txn.open_table(NATIVE_METADATA_TABLE)?;
            if metadata.get("store")?.is_none() {
                let row = NativeStoreMetadata::new(format!("store_{:016x}", rand::random::<u64>()));
                let json = serde_json::to_string(&row)?;
                metadata.insert("store", json.as_str())?;
            }
            let _ = write_txn.open_table(NATIVE_TASKS_TABLE)?;
            let _ = write_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
        }
        write_txn.commit()?;

        Ok(Self { db: Arc::new(db) })
    }

    // ── Jobs ──────────────────────────────────────────────────────────

    /// Insert or update a job.
    pub fn put_job(&self, job: &Job) -> Result<()> {
        let json = serde_json::to_string(job)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(JOBS_TABLE)?;
            table.insert(job.gid.as_raw(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve a job by GID.
    pub fn get_job(&self, gid: Gid) -> Result<Option<Job>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(JOBS_TABLE)?;
        match table.get(gid.as_raw())? {
            Some(guard) => {
                let job: Job = serde_json::from_str(guard.value())?;
                Ok(Some(job))
            }
            None => Ok(None),
        }
    }

    /// Remove a job by GID.
    pub fn remove_job(&self, gid: Gid) -> Result<bool> {
        let write_txn = self.db.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(JOBS_TABLE)?;
            table.remove(gid.as_raw())?.is_some()
        };
        write_txn.commit()?;
        Ok(removed)
    }

    /// List all jobs.
    pub fn list_jobs(&self) -> Result<Vec<Job>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(JOBS_TABLE)?;
        let mut jobs = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            let job: Job = serde_json::from_str(value.value())?;
            jobs.push(job);
        }
        Ok(jobs)
    }

    // ── Segments ──────────────────────────────────────────────────────

    /// Insert or update a segment state.
    pub fn put_segment(&self, gid: Gid, seg_id: u32, state: &SegmentState) -> Result<()> {
        let json = serde_json::to_string(state)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SEGMENTS_TABLE)?;
            table.insert((gid.as_raw(), seg_id), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve a segment state.
    pub fn get_segment(&self, gid: Gid, seg_id: u32) -> Result<Option<SegmentState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SEGMENTS_TABLE)?;
        match table.get((gid.as_raw(), seg_id))? {
            Some(guard) => {
                let state: SegmentState = serde_json::from_str(guard.value())?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    /// List all segments for a given job.
    pub fn list_segments(&self, gid: Gid) -> Result<Vec<(u32, SegmentState)>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SEGMENTS_TABLE)?;
        let mut segments = Vec::new();
        // Scan full range — filter by gid prefix.
        for entry in table.iter()? {
            let (key, value) = entry?;
            let (job_gid, seg_id) = key.value();
            if job_gid == gid.as_raw() {
                let state: SegmentState = serde_json::from_str(value.value())?;
                segments.push((seg_id, state));
            }
        }
        Ok(segments)
    }

    /// Remove all segments for a given job.
    pub fn remove_segments(&self, gid: Gid) -> Result<u32> {
        let write_txn = self.db.begin_write()?;
        let mut count = 0u32;
        {
            let mut table = write_txn.open_table(SEGMENTS_TABLE)?;
            // Collect keys to remove.
            let keys: Vec<(u64, u32)> = {
                let mut keys = Vec::new();
                for entry in table.iter()? {
                    let (key, _) = entry?;
                    let (job_gid, seg_id) = key.value();
                    if job_gid == gid.as_raw() {
                        keys.push((job_gid, seg_id));
                    }
                }
                keys
            };
            for key in keys {
                table.remove(key)?;
                count += 1;
            }
        }
        write_txn.commit()?;
        Ok(count)
    }

    /// Insert or update a segment state by native task id.
    pub fn put_native_segment(
        &self,
        task_id: &TaskId,
        seg_id: u32,
        state: &SegmentState,
    ) -> Result<()> {
        let json = serde_json::to_string(state)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
            table.insert((task_id.as_str(), seg_id), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve a segment state by native task id.
    pub fn get_native_segment(
        &self,
        task_id: &TaskId,
        seg_id: u32,
    ) -> Result<Option<SegmentState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
        match table.get((task_id.as_str(), seg_id))? {
            Some(guard) => {
                let state: SegmentState = serde_json::from_str(guard.value())?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    /// List all segments for a native task.
    pub fn list_native_segments(&self, task_id: &TaskId) -> Result<Vec<(u32, SegmentState)>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
        let mut segments = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            let (row_task_id, seg_id) = key.value();
            if row_task_id == task_id.as_str() {
                let state: SegmentState = serde_json::from_str(value.value())?;
                segments.push((seg_id, state));
            }
        }
        Ok(segments)
    }

    /// Remove all segments for a native task.
    pub fn remove_native_segments(&self, task_id: &TaskId) -> Result<u32> {
        let write_txn = self.db.begin_write()?;
        let mut count = 0u32;
        {
            let mut table = write_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
            let keys: Vec<(String, u32)> = {
                let mut keys = Vec::new();
                for entry in table.iter()? {
                    let (key, _) = entry?;
                    let (row_task_id, seg_id) = key.value();
                    if row_task_id == task_id.as_str() {
                        keys.push((row_task_id.to_string(), seg_id));
                    }
                }
                keys
            };
            for (row_task_id, seg_id) in keys {
                table.remove((row_task_id.as_str(), seg_id))?;
                count += 1;
            }
        }
        write_txn.commit()?;
        Ok(count)
    }

    // ── Job Options ───────────────────────────────────────────────────

    /// Insert or update job options.
    pub fn put_job_options(&self, gid: Gid, opts: &JobOptions) -> Result<()> {
        let json = serde_json::to_string(opts)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(JOB_OPTIONS_TABLE)?;
            table.insert(gid.as_raw(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve job options.
    pub fn get_job_options(&self, gid: Gid) -> Result<Option<JobOptions>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(JOB_OPTIONS_TABLE)?;
        match table.get(gid.as_raw())? {
            Some(guard) => {
                let opts: JobOptions = serde_json::from_str(guard.value())?;
                Ok(Some(opts))
            }
            None => Ok(None),
        }
    }

    // ── Global State ──────────────────────────────────────────────────

    /// Set a global state key-value pair.
    pub fn put_global(&self, key: &str, value: &str) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(GLOBAL_STATE_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get a global state value.
    pub fn get_global(&self, key: &str) -> Result<Option<String>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(GLOBAL_STATE_TABLE)?;
        match table.get(key)? {
            Some(guard) => Ok(Some(guard.value().to_string())),
            None => Ok(None),
        }
    }

    // ── Native Store Schema ──────────────────────────────────────────

    /// Return the native store metadata row.
    pub fn native_metadata(&self) -> Result<NativeStoreMetadata> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_METADATA_TABLE)?;
        let value = table
            .get("store")?
            .context("native store metadata is missing")?;
        let metadata: NativeStoreMetadata = serde_json::from_str(value.value())?;
        Ok(metadata)
    }

    /// Insert or update a native task row.
    pub fn put_native_task(&self, row: &NativeTaskRow) -> Result<()> {
        row.validate_version()
            .context("unsupported native task row version")?;
        let json = serde_json::to_string(row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(NATIVE_TASKS_TABLE)?;
            table.insert(row.task_id.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve a native task row by task id.
    pub fn get_native_task(&self, task_id: &TaskId) -> Result<Option<NativeTaskRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_TASKS_TABLE)?;
        match table.get(task_id.as_str())? {
            Some(guard) => {
                let row: NativeTaskRow = serde_json::from_str(guard.value())?;
                row.validate_version()
                    .context("unsupported native task row version")?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// List all native task rows.
    pub fn list_native_tasks(&self) -> Result<Vec<NativeTaskRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_TASKS_TABLE)?;
        let mut rows = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            let row: NativeTaskRow = serde_json::from_str(value.value())?;
            row.validate_version()
                .context("unsupported native task row version")?;
            rows.push(row);
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Gid;
    use crate::native::TaskLifecycle;
    use crate::segment::SegmentStatus;
    use std::path::PathBuf;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let store = Store::open(&db_path).unwrap();
        (store, dir)
    }

    #[test]
    fn store_opens_and_creates_tables() {
        let (_store, _dir) = temp_store();
        // If we got here, tables were created successfully.
    }

    #[test]
    fn put_get_job_roundtrips() {
        let (store, _dir) = temp_store();
        let job = Job::new_range(
            vec!["https://example.com/file.zip".into()],
            PathBuf::from("/tmp/file.zip"),
        );
        let gid = job.gid;

        store.put_job(&job).unwrap();
        let recovered = store.get_job(gid).unwrap().expect("job should exist");

        assert_eq!(recovered.gid, gid);
        assert_eq!(recovered.uris, job.uris);
        assert_eq!(recovered.status, job.status);
    }

    #[test]
    fn get_nonexistent_job_returns_none() {
        let (store, _dir) = temp_store();
        let result = store.get_job(Gid::from_raw(99999)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn remove_job_works() {
        let (store, _dir) = temp_store();
        let job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        let gid = job.gid;

        store.put_job(&job).unwrap();
        assert!(store.remove_job(gid).unwrap());
        assert!(store.get_job(gid).unwrap().is_none());
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let (store, _dir) = temp_store();
        assert!(!store.remove_job(Gid::from_raw(99999)).unwrap());
    }

    #[test]
    fn list_jobs_returns_all() {
        let (store, _dir) = temp_store();
        let j1 = Job::new_range(vec!["a".into()], PathBuf::from("/a"));
        let j2 = Job::new_bt(vec!["b".into()], PathBuf::from("/b"));

        store.put_job(&j1).unwrap();
        store.put_job(&j2).unwrap();

        let jobs = store.list_jobs().unwrap();
        assert_eq!(jobs.len(), 2);
    }

    #[test]
    fn get_job_restores_legacy_row_missing_new_fields() {
        let (store, _dir) = temp_store();
        let legacy_json = serde_json::json!({
            "gid": 7,
            "kind": "bt",
            "status": "waiting",
            "uris": ["magnet:?xt=urn:btih:abc123"],
            "out_path": "/tmp/bt-download",
            "total_size": null,
            "downloaded": 0,
            "upload_speed": 0,
            "download_speed": 0,
            "created_at": "2026-04-10T00:00:00Z",
            "error_msg": null,
            "options": {
                "max_connections": 16,
                "max_download_limit": 0,
                "max_upload_limit": 0,
                "dir": null,
                "out": null,
                "headers": [],
                "http_user": null,
                "http_passwd": null,
                "checksum": null
            }
        })
        .to_string();

        let write_txn = store.db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(JOBS_TABLE).unwrap();
            table.insert(7, legacy_json.as_str()).unwrap();
        }
        write_txn.commit().unwrap();

        let recovered = store
            .get_job(Gid::from_raw(7))
            .unwrap()
            .expect("legacy job should deserialize");

        assert_eq!(recovered.gid, Gid::from_raw(7));
        assert_eq!(recovered.connections, 0);
        assert_eq!(recovered.options.bt_selected_files, None);
        assert_eq!(recovered.status, crate::job::Status::Waiting);
        assert!(recovered.bt_files.is_none());
    }

    #[test]
    fn list_jobs_restores_legacy_rows_missing_new_fields() {
        let (store, _dir) = temp_store();
        let legacy_json = serde_json::json!({
            "gid": 9,
            "kind": "range",
            "status": "active",
            "uris": ["https://example.com/file.zip"],
            "out_path": "/tmp/file.zip",
            "total_size": 1024,
            "downloaded": 256,
            "upload_speed": 0,
            "download_speed": 128,
            "created_at": "2026-04-10T00:00:00Z",
            "error_msg": null,
            "options": {
                "max_connections": 8,
                "max_download_limit": 0,
                "max_upload_limit": 0,
                "dir": null,
                "out": "file.zip",
                "headers": [],
                "http_user": null,
                "http_passwd": null,
                "checksum": null
            }
        })
        .to_string();

        let write_txn = store.db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(JOBS_TABLE).unwrap();
            table.insert(9, legacy_json.as_str()).unwrap();
        }
        write_txn.commit().unwrap();

        let jobs = store.list_jobs().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].gid, Gid::from_raw(9));
        assert_eq!(jobs[0].connections, 0);
        assert_eq!(jobs[0].options.bt_selected_files, None);
        assert_eq!(jobs[0].downloaded, 256);
    }

    #[test]
    fn put_get_segment_roundtrips() {
        let (store, _dir) = temp_store();
        let gid = Gid::from_raw(1);
        let task_id = TaskId::new();
        let seg = SegmentState {
            start: 0,
            end: 1000,
            downloaded: 500,
            etag: Some("abc".into()),
            status: SegmentStatus::Active,
        };

        store.put_segment(gid, 0, &seg).unwrap();
        let recovered = store.get_segment(gid, 0).unwrap().expect("segment exists");

        assert_eq!(recovered.start, 0);
        assert_eq!(recovered.end, 1000);
        assert_eq!(recovered.downloaded, 500);
        assert_eq!(recovered.etag.as_deref(), Some("abc"));

        store.put_native_segment(&task_id, 0, &seg).unwrap();
        let native_recovered = store
            .get_native_segment(&task_id, 0)
            .unwrap()
            .expect("native segment exists");
        assert_eq!(native_recovered.start, 0);
        assert_eq!(native_recovered.end, 1000);
        assert_eq!(native_recovered.downloaded, 500);
        assert_eq!(native_recovered.etag.as_deref(), Some("abc"));
    }

    #[test]
    fn native_segments_are_keyed_by_task_id() {
        let (store, _dir) = temp_store();
        let task_a = TaskId::new();
        let task_b = TaskId::new();

        let seg = SegmentState {
            start: 0,
            end: 100,
            downloaded: 0,
            etag: None,
            status: SegmentStatus::Pending,
        };

        store.put_native_segment(&task_a, 0, &seg).unwrap();
        store.put_native_segment(&task_a, 1, &seg).unwrap();
        store.put_native_segment(&task_b, 0, &seg).unwrap();

        assert_eq!(store.list_native_segments(&task_a).unwrap().len(), 2);
        assert_eq!(store.list_native_segments(&task_b).unwrap().len(), 1);
        assert_eq!(store.remove_native_segments(&task_a).unwrap(), 2);
        assert!(store.list_native_segments(&task_a).unwrap().is_empty());
        assert_eq!(store.list_native_segments(&task_b).unwrap().len(), 1);
    }

    #[test]
    fn list_segments_filters_by_gid() {
        let (store, _dir) = temp_store();
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);

        let s1 = SegmentState {
            start: 0,
            end: 100,
            downloaded: 0,
            etag: None,
            status: SegmentStatus::Pending,
        };
        let s2 = SegmentState {
            start: 100,
            end: 200,
            downloaded: 0,
            etag: None,
            status: SegmentStatus::Pending,
        };

        store.put_segment(g1, 0, &s1).unwrap();
        store.put_segment(g1, 1, &s2).unwrap();
        store.put_segment(g2, 0, &s1).unwrap();

        let segs = store.list_segments(g1).unwrap();
        assert_eq!(segs.len(), 2);

        let segs2 = store.list_segments(g2).unwrap();
        assert_eq!(segs2.len(), 1);
    }

    #[test]
    fn remove_segments_cleans_all_for_gid() {
        let (store, _dir) = temp_store();
        let gid = Gid::from_raw(1);
        let seg = SegmentState {
            start: 0,
            end: 100,
            downloaded: 0,
            etag: None,
            status: SegmentStatus::Pending,
        };
        store.put_segment(gid, 0, &seg).unwrap();
        store.put_segment(gid, 1, &seg).unwrap();

        let removed = store.remove_segments(gid).unwrap();
        assert_eq!(removed, 2);
        assert!(store.list_segments(gid).unwrap().is_empty());
    }

    #[test]
    fn put_get_job_options_roundtrips() {
        let (store, _dir) = temp_store();
        let gid = Gid::from_raw(1);
        let opts = JobOptions {
            max_connections: 8,
            out: Some("custom.zip".into()),
            ..JobOptions::default()
        };

        store.put_job_options(gid, &opts).unwrap();
        let recovered = store.get_job_options(gid).unwrap().expect("opts exist");

        assert_eq!(recovered.max_connections, 8);
        assert_eq!(recovered.out.as_deref(), Some("custom.zip"));
    }

    #[test]
    fn global_state_put_get() {
        let (store, _dir) = temp_store();
        store.put_global("next_gid", "42").unwrap();
        let val = store.get_global("next_gid").unwrap().expect("exists");
        assert_eq!(val, "42");
    }

    #[test]
    fn global_state_missing_returns_none() {
        let (store, _dir) = temp_store();
        assert!(store.get_global("nonexistent").unwrap().is_none());
    }

    #[test]
    fn global_state_overwrite() {
        let (store, _dir) = temp_store();
        store.put_global("key", "v1").unwrap();
        store.put_global("key", "v2").unwrap();
        let val = store.get_global("key").unwrap().unwrap();
        assert_eq!(val, "v2");
    }

    #[test]
    fn native_metadata_is_created_when_store_opens() {
        let (store, _dir) = temp_store();

        let metadata = store.native_metadata().unwrap();

        assert_eq!(
            metadata.schema_version,
            NativeStoreMetadata::CURRENT_SCHEMA_VERSION
        );
        assert!(metadata.store_id.starts_with("store_"));
    }

    #[test]
    fn native_task_rows_roundtrip_by_task_id() {
        let (store, _dir) = temp_store();
        let task_id = TaskId::new();
        let row = NativeTaskRow::new(task_id.clone(), TaskLifecycle::Queued);

        store.put_native_task(&row).unwrap();
        let recovered = store
            .get_native_task(&task_id)
            .unwrap()
            .expect("native task row");

        assert_eq!(recovered.task_id, task_id);
        assert_eq!(recovered.lifecycle, TaskLifecycle::Queued);
        assert_eq!(recovered.row_version, NativeTaskRow::CURRENT_ROW_VERSION);
    }

    #[test]
    fn list_native_task_rows_returns_all_rows() {
        let (store, _dir) = temp_store();
        let first = NativeTaskRow::new(TaskId::new(), TaskLifecycle::Queued);
        let second = NativeTaskRow::new(TaskId::new(), TaskLifecycle::Paused);

        store.put_native_task(&first).unwrap();
        store.put_native_task(&second).unwrap();

        let rows = store.list_native_tasks().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.task_id == first.task_id));
        assert!(rows.iter().any(|row| row.task_id == second.task_id));
    }

    /// Integration test: simulate checkpoint + resume cycle.
    /// This validates the full crash recovery flow.
    #[test]
    fn segment_checkpoint_resume_cycle() {
        use crate::segment::{init_segment_states, plan_segments};

        let (store, _dir) = temp_store();
        let gid = Gid::from_raw(42);
        let total_size = 10_000u64;
        let num_segments = 4;

        // Plan segments.
        let ranges = plan_segments(total_size, num_segments);
        let segments = init_segment_states(&ranges);
        assert_eq!(segments.len(), 4);

        // Simulate partial download: segments 0 and 1 done, segment 2 partial.
        store
            .put_segment(
                gid,
                0,
                &SegmentState {
                    start: segments[0].start,
                    end: segments[0].end,
                    downloaded: segments[0].size(),
                    etag: None,
                    status: SegmentStatus::Done,
                },
            )
            .unwrap();
        store
            .put_segment(
                gid,
                1,
                &SegmentState {
                    start: segments[1].start,
                    end: segments[1].end,
                    downloaded: segments[1].size(),
                    etag: None,
                    status: SegmentStatus::Done,
                },
            )
            .unwrap();
        store
            .put_segment(
                gid,
                2,
                &SegmentState {
                    start: segments[2].start,
                    end: segments[2].end,
                    downloaded: 500,
                    etag: Some("abc123".into()),
                    status: SegmentStatus::Active,
                },
            )
            .unwrap();

        // Now simulate resume: re-plan segments and merge persisted state.
        let fresh_ranges = plan_segments(total_size, num_segments);
        let mut fresh_segments = init_segment_states(&fresh_ranges);

        let persisted = store.list_segments(gid).unwrap();
        assert_eq!(persisted.len(), 3); // Only 3 segments were checkpointed.

        for (seg_id, persisted_state) in &persisted {
            if let Some(seg) = fresh_segments.get_mut(*seg_id as usize) {
                if persisted_state.downloaded > 0 && persisted_state.downloaded <= seg.size() {
                    seg.downloaded = persisted_state.downloaded;
                }
            }
        }

        // Verify merged state.
        assert_eq!(fresh_segments[0].downloaded, fresh_segments[0].size()); // Done.
        assert_eq!(fresh_segments[1].downloaded, fresh_segments[1].size()); // Done.
        assert_eq!(fresh_segments[2].downloaded, 500); // Partial.
        assert_eq!(fresh_segments[3].downloaded, 0); // Not checkpointed.
    }

    /// Test that segment checkpoint updates are idempotent (overwrite works).
    #[test]
    fn segment_checkpoint_idempotent_update() {
        let (store, _dir) = temp_store();
        let gid = Gid::from_raw(1);

        let seg_v1 = SegmentState {
            start: 0,
            end: 1000,
            downloaded: 100,
            etag: None,
            status: SegmentStatus::Active,
        };
        store.put_segment(gid, 0, &seg_v1).unwrap();

        // Update with more progress.
        let seg_v2 = SegmentState {
            start: 0,
            end: 1000,
            downloaded: 750,
            etag: None,
            status: SegmentStatus::Active,
        };
        store.put_segment(gid, 0, &seg_v2).unwrap();

        // Should get the latest.
        let recovered = store.get_segment(gid, 0).unwrap().unwrap();
        assert_eq!(recovered.downloaded, 750);
    }
}
