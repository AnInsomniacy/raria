// raria-core: Job registry — in-memory index of all jobs.
//
// The registry provides thread-safe access to jobs, enforces GID uniqueness,
// and supports filtering by status.

use crate::job::{Gid, Job, Status};
use crate::native::TaskId;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Thread-safe registry of all download jobs.
#[derive(Debug, Clone)]
pub struct JobRegistry {
    inner: Arc<RwLock<RegistryInner>>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    jobs: HashMap<Gid, Job>,
    by_task_id: HashMap<TaskId, Gid>,
}

impl JobRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(RegistryInner::default())),
        }
    }

    /// Insert a job into the registry. Returns an error if the GID already exists.
    pub fn insert(&self, job: Job) -> Result<Gid, RegistryError> {
        let mut inner = self.inner.write();
        let gid = job.gid;
        if inner.jobs.contains_key(&gid) {
            return Err(RegistryError::DuplicateGid(gid));
        }
        inner.by_task_id.insert(job.task_id.clone(), gid);
        inner.jobs.insert(gid, job);
        Ok(gid)
    }

    /// Look up a job by GID. Returns a clone.
    pub fn get(&self, gid: Gid) -> Option<Job> {
        let inner = self.inner.read();
        inner.jobs.get(&gid).cloned()
    }

    /// Look up a job by native task id. Returns a clone.
    pub fn get_by_task_id(&self, task_id: &TaskId) -> Option<Job> {
        let inner = self.inner.read();
        inner
            .by_task_id
            .get(task_id)
            .and_then(|gid| inner.jobs.get(gid))
            .cloned()
    }

    /// Resolve a native task id to the runtime bridge id.
    pub fn gid_for_task_id(&self, task_id: &TaskId) -> Option<Gid> {
        let inner = self.inner.read();
        inner.by_task_id.get(task_id).copied()
    }

    /// Remove a job from the registry. Returns the removed job if it existed.
    pub fn remove(&self, gid: Gid) -> Option<Job> {
        let mut inner = self.inner.write();
        let removed = inner.jobs.remove(&gid)?;
        inner.by_task_id.remove(&removed.task_id);
        Some(removed)
    }

    /// Update a job in the registry by applying a closure.
    ///
    /// Returns `None` if the GID does not exist.
    pub fn update<F, R>(&self, gid: Gid, f: F) -> Option<R>
    where
        F: FnOnce(&mut Job) -> R,
    {
        let mut inner = self.inner.write();
        let job = inner.jobs.get_mut(&gid)?;
        let old_task_id = job.task_id.clone();
        let result = f(job);
        let new_task_id = job.task_id.clone();
        if old_task_id != new_task_id {
            inner.by_task_id.remove(&old_task_id);
            inner.by_task_id.insert(new_task_id, gid);
        }
        Some(result)
    }

    /// Return the number of jobs in the registry.
    pub fn len(&self) -> usize {
        let inner = self.inner.read();
        inner.jobs.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// List all jobs with a given status.
    pub fn by_status(&self, status: Status) -> Vec<Job> {
        let inner = self.inner.read();
        inner
            .jobs
            .values()
            .filter(|j| j.status == status)
            .cloned()
            .collect()
    }

    /// List all GIDs.
    pub fn all_gids(&self) -> Vec<Gid> {
        let inner = self.inner.read();
        inner.jobs.keys().copied().collect()
    }

    /// Snapshot: return clones of all jobs.
    pub fn snapshot(&self) -> Vec<Job> {
        let inner = self.inner.read();
        inner.jobs.values().cloned().collect()
    }

    /// Load jobs from a vector (e.g., restored from persistence).
    pub fn load_from(&self, jobs: Vec<Job>) {
        let mut inner = self.inner.write();
        for job in jobs {
            inner.by_task_id.insert(job.task_id.clone(), job.gid);
            inner.jobs.insert(job.gid, job);
        }
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur in registry operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RegistryError {
    /// A job with this GID already exists in the registry.
    #[error("duplicate GID: {0}")]
    DuplicateGid(Gid),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{Gid, Job, Status};
    use std::path::PathBuf;

    fn make_job(uri: &str) -> Job {
        Job::new_range(vec![uri.into()], PathBuf::from("/tmp/f"))
    }

    #[test]
    fn new_registry_is_empty() {
        let reg = JobRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn insert_and_get() {
        let reg = JobRegistry::new();
        let job = make_job("https://a.com/f");
        let gid = job.gid;
        let task_id = job.task_id.clone();

        reg.insert(job).unwrap();
        assert_eq!(reg.len(), 1);

        let retrieved = reg.get(gid).expect("job should exist");
        assert_eq!(retrieved.gid, gid);
        assert_eq!(retrieved.uris[0], "https://a.com/f");
        assert_eq!(reg.gid_for_task_id(&task_id), Some(gid));
        assert_eq!(reg.get_by_task_id(&task_id).expect("task").gid, gid);
    }

    #[test]
    fn task_id_index_updates_on_update_remove_and_load() {
        let reg = JobRegistry::new();
        let first = make_job("https://a.com/f");
        let first_gid = first.gid;
        let first_task_id = first.task_id.clone();
        let replacement_task_id = crate::native::TaskId::new();

        reg.insert(first).unwrap();
        reg.update(first_gid, |job| {
            job.task_id = replacement_task_id.clone();
        });

        assert_eq!(reg.gid_for_task_id(&first_task_id), None);
        assert_eq!(reg.gid_for_task_id(&replacement_task_id), Some(first_gid));

        reg.remove(first_gid).expect("removed");
        assert_eq!(reg.gid_for_task_id(&replacement_task_id), None);

        let loaded = make_job("https://b.com/f");
        let loaded_gid = loaded.gid;
        let loaded_task_id = loaded.task_id.clone();
        reg.load_from(vec![loaded]);

        assert_eq!(reg.gid_for_task_id(&loaded_task_id), Some(loaded_gid));
    }

    #[test]
    fn insert_duplicate_gid_fails() {
        let reg = JobRegistry::new();
        let gid = Gid::from_raw(42);
        let mut job1 = make_job("a");
        job1.gid = gid;
        let mut job2 = make_job("b");
        job2.gid = gid;

        reg.insert(job1).unwrap();
        let err = reg.insert(job2).unwrap_err();
        assert!(matches!(err, RegistryError::DuplicateGid(g) if g == gid));
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let reg = JobRegistry::new();
        assert!(reg.get(Gid::from_raw(99999)).is_none());
    }

    #[test]
    fn remove_returns_job() {
        let reg = JobRegistry::new();
        let job = make_job("x");
        let gid = job.gid;
        reg.insert(job).unwrap();

        let removed = reg.remove(gid).expect("should remove");
        assert_eq!(removed.gid, gid);
        assert!(reg.is_empty());
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let reg = JobRegistry::new();
        assert!(reg.remove(Gid::from_raw(123)).is_none());
    }

    #[test]
    fn update_modifies_in_place() {
        let reg = JobRegistry::new();
        let job = make_job("y");
        let gid = job.gid;
        reg.insert(job).unwrap();

        let result = reg.update(gid, |j| {
            j.downloaded = 1000;
            j.downloaded
        });
        assert_eq!(result, Some(1000));

        let retrieved = reg.get(gid).unwrap();
        assert_eq!(retrieved.downloaded, 1000);
    }

    #[test]
    fn update_nonexistent_returns_none() {
        let reg = JobRegistry::new();
        let result = reg.update(Gid::from_raw(123), |j| j.downloaded = 1);
        assert!(result.is_none());
    }

    #[test]
    fn by_status_filters_correctly() {
        let reg = JobRegistry::new();

        let j1 = make_job("a"); // Waiting
        let j2 = make_job("b"); // Waiting
        let mut j3 = make_job("c");
        j3.status = Status::Active;

        reg.insert(j1).unwrap();
        reg.insert(j2).unwrap();
        reg.insert(j3).unwrap();

        let waiting = reg.by_status(Status::Waiting);
        assert_eq!(waiting.len(), 2);

        let active = reg.by_status(Status::Active);
        assert_eq!(active.len(), 1);

        let paused = reg.by_status(Status::Paused);
        assert!(paused.is_empty());
    }

    #[test]
    fn snapshot_returns_all() {
        let reg = JobRegistry::new();
        reg.insert(make_job("a")).unwrap();
        reg.insert(make_job("b")).unwrap();

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn load_from_restores_jobs() {
        let reg = JobRegistry::new();
        let j1 = make_job("a");
        let j2 = make_job("b");
        let gid1 = j1.gid;
        let gid2 = j2.gid;

        reg.load_from(vec![j1, j2]);
        assert_eq!(reg.len(), 2);
        assert!(reg.get(gid1).is_some());
        assert!(reg.get(gid2).is_some());
    }

    #[test]
    fn all_gids_returns_all_keys() {
        let reg = JobRegistry::new();
        let j1 = make_job("a");
        let j2 = make_job("b");
        let gid1 = j1.gid;
        let gid2 = j2.gid;
        reg.insert(j1).unwrap();
        reg.insert(j2).unwrap();

        let mut gids = reg.all_gids();
        gids.sort_by_key(|g| g.as_raw());
        assert_eq!(gids.len(), 2);
        assert!(gids.contains(&gid1));
        assert!(gids.contains(&gid2));
    }
}
