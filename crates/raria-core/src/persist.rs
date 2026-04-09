// raria-core: Persistence layer using redb.
//
// This module implements the storage backend for job metadata, segment state,
// job options, and global state using redb — an embedded key-value store.

use crate::config::JobOptions;
use crate::job::{Gid, Job};
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Gid;
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
    fn put_get_segment_roundtrips() {
        let (store, _dir) = temp_store();
        let gid = Gid::from_raw(1);
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
        let mut opts = JobOptions::default();
        opts.max_connections = 8;
        opts.out = Some("custom.zip".into());

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
}
