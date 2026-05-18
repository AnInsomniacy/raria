// raria-core: Scheduler — manages job queue ordering and concurrency.
//
// The scheduler controls which jobs are active, how many run concurrently,
// and handles the waiting → active state transitions.

use crate::job::{Gid, Status};
use crate::native::{NativeTaskIndex, TaskId};
use crate::registry::JobRegistry;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Controls the execution queue for download jobs.
#[derive(Debug)]
pub struct Scheduler {
    /// Maximum number of concurrently active jobs.
    max_concurrent: AtomicU32,
    /// Ordered queue of waiting native task ids. Front = next to activate.
    queue: Arc<RwLock<VecDeque<TaskId>>>,
}

impl Clone for Scheduler {
    fn clone(&self) -> Self {
        Self {
            max_concurrent: AtomicU32::new(self.max_concurrent.load(Ordering::Relaxed)),
            queue: Arc::clone(&self.queue),
        }
    }
}

impl Scheduler {
    /// Create a new scheduler with the given concurrency limit.
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            max_concurrent: AtomicU32::new(max_concurrent.max(1)),
            queue: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    /// Enqueue a native task id at the back of the waiting queue.
    pub fn enqueue_task(&self, task_id: TaskId) {
        let mut queue = self.queue.write();
        queue.push_back(task_id);
    }

    /// Enqueue a native task id at a specific position.
    pub fn enqueue_task_at(&self, task_id: TaskId, position: usize) {
        let mut queue = self.queue.write();
        let pos = position.min(queue.len());
        queue.insert(pos, task_id);
    }

    /// Remove a native task id from the waiting queue.
    pub fn dequeue_task(&self, task_id: &TaskId) -> bool {
        let mut queue = self.queue.write();
        if let Some(pos) = queue.iter().position(|queued| queued == task_id) {
            queue.remove(pos);
            true
        } else {
            false
        }
    }

    /// Return the current native task queue.
    pub fn waiting_task_queue(&self) -> Vec<TaskId> {
        let queue = self.queue.read();
        queue.iter().cloned().collect()
    }

    /// The number of jobs in the waiting queue.
    pub fn queue_len(&self) -> usize {
        let queue = self.queue.read();
        queue.len()
    }

    /// Move a native task id to a different position in the queue.
    pub fn change_task_position(
        &self,
        task_id: TaskId,
        pos: i32,
        how: crate::engine::PositionHow,
    ) -> anyhow::Result<usize> {
        let mut queue = self.queue.write();
        change_task_position_locked(&mut queue, task_id, pos, how)
    }
}

fn change_task_position_locked(
    queue: &mut VecDeque<TaskId>,
    task_id: TaskId,
    pos: i32,
    how: crate::engine::PositionHow,
) -> anyhow::Result<usize> {
    use crate::engine::PositionHow;
    let cur_pos = queue
        .iter()
        .position(|queued| *queued == task_id)
        .ok_or_else(|| anyhow::anyhow!("task {} not in queue", task_id.as_str()))?;
    queue.remove(cur_pos);
    let len = queue.len();
    let new_pos = match how {
        PositionHow::Set => (pos.max(0) as usize).min(len),
        PositionHow::Cur => {
            let target = cur_pos as i64 + pos as i64;
            target.max(0).min(len as i64) as usize
        }
        PositionHow::End => {
            let target = len as i64 + pos as i64;
            target.max(0).min(len as i64) as usize
        }
    };
    queue.insert(new_pos, task_id);
    Ok(new_pos)
}

impl Scheduler {
    /// Determine which runtime bridge ids should be promoted from Waiting to Active.
    pub fn jobs_to_activate(
        &self,
        registry: &JobRegistry,
        native_task_index: &NativeTaskIndex,
    ) -> Vec<Gid> {
        let max = self.max_concurrent.load(Ordering::Relaxed);
        let active_count = registry.by_status(Status::Active).len() as u32;
        if active_count >= max {
            return Vec::new();
        }

        let slots = (max - active_count) as usize;
        let queue = self.queue.read();
        queue
            .iter()
            .take(slots)
            .filter_map(|task_id| {
                registry
                    .gid_for_task_id(task_id)
                    .or_else(|| native_task_index.gid_for_task_id(task_id))
            })
            .collect()
    }

    /// Determine which native task ids should be promoted from queued to running.
    pub fn native_tasks_to_activate(&self, registry: &JobRegistry) -> Vec<TaskId> {
        let max = self.max_concurrent.load(Ordering::Relaxed);
        let active_count = registry.by_status(Status::Active).len() as u32;
        if active_count >= max {
            return Vec::new();
        }

        let slots = (max - active_count) as usize;
        let queue = self.queue.read();
        queue
            .iter()
            .take(slots)
            .filter(|task_id| registry.get_by_task_id(task_id).is_some())
            .cloned()
            .collect()
    }

    /// The maximum number of concurrent downloads.
    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent.load(Ordering::Relaxed)
    }

    /// Update the maximum concurrency.
    /// Update the maximum concurrency (thread-safe, no &mut needed).
    pub fn set_max_concurrent(&self, max: u32) {
        self.max_concurrent.store(max.max(1), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{Job, Status};
    use std::path::PathBuf;

    fn make_job(uri: &str) -> Job {
        Job::new_range(vec![uri.into()], PathBuf::from("/tmp/f"))
    }

    #[test]
    fn new_scheduler_has_empty_queue() {
        let sched = Scheduler::new(5);
        assert_eq!(sched.queue_len(), 0);
        assert_eq!(sched.max_concurrent(), 5);
    }

    #[test]
    fn min_concurrency_is_one() {
        let sched = Scheduler::new(0);
        assert_eq!(sched.max_concurrent(), 1);
    }

    #[test]
    fn enqueue_adds_to_back() {
        let sched = Scheduler::new(5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        sched.enqueue_task(task1.clone());
        sched.enqueue_task(task2.clone());

        let queue = sched.waiting_task_queue();
        assert_eq!(queue, vec![task1, task2]);
    }

    #[test]
    fn enqueue_at_inserts_at_position() {
        let sched = Scheduler::new(5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();

        sched.enqueue_task(task1.clone());
        sched.enqueue_task(task3.clone());
        sched.enqueue_task_at(task2.clone(), 1);

        let queue = sched.waiting_task_queue();
        assert_eq!(queue, vec![task1, task2, task3]);
    }

    #[test]
    fn enqueue_at_beyond_length_appends() {
        let sched = Scheduler::new(5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        sched.enqueue_task(task1.clone());
        sched.enqueue_task_at(task2.clone(), 100);

        let queue = sched.waiting_task_queue();
        assert_eq!(queue, vec![task1, task2]);
    }

    #[test]
    fn dequeue_removes_task_id() {
        let sched = Scheduler::new(5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        sched.enqueue_task(task1.clone());
        sched.enqueue_task(task2.clone());

        assert!(sched.dequeue_task(&task1));
        assert_eq!(sched.waiting_task_queue(), vec![task2]);
    }

    #[test]
    fn dequeue_nonexistent_returns_false() {
        let sched = Scheduler::new(5);
        assert!(!sched.dequeue_task(&TaskId::new()));
    }

    #[test]
    fn change_position_moves_task_id() {
        let sched = Scheduler::new(5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();
        sched.enqueue_task(task1.clone());
        sched.enqueue_task(task2.clone());
        sched.enqueue_task(task3.clone());

        use crate::engine::PositionHow;
        let new_pos = sched
            .change_task_position(task3.clone(), 0, PositionHow::Set)
            .unwrap();
        assert_eq!(new_pos, 0);
        assert_eq!(sched.waiting_task_queue(), vec![task3, task1, task2]);
    }

    #[test]
    fn change_position_nonexistent_returns_error() {
        let sched = Scheduler::new(5);
        use crate::engine::PositionHow;
        let result = sched.change_task_position(TaskId::new(), 0, PositionHow::Set);
        assert!(result.is_err());
    }

    #[test]
    fn change_position_cur_moves_relative() {
        let sched = Scheduler::new(5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();
        sched.enqueue_task(task1.clone());
        sched.enqueue_task(task2.clone());
        sched.enqueue_task(task3.clone());

        use crate::engine::PositionHow;
        let new_pos = sched
            .change_task_position(task1.clone(), 1, PositionHow::Cur)
            .unwrap();
        assert_eq!(new_pos, 1);
        assert_eq!(sched.waiting_task_queue(), vec![task2, task1, task3]);
    }

    #[test]
    fn change_position_end_moves_from_tail() {
        let sched = Scheduler::new(5);
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();
        sched.enqueue_task(task1.clone());
        sched.enqueue_task(task2.clone());
        sched.enqueue_task(task3.clone());

        use crate::engine::PositionHow;
        let new_pos = sched
            .change_task_position(task1.clone(), 0, PositionHow::End)
            .unwrap();
        assert_eq!(new_pos, 2);
        assert_eq!(sched.waiting_task_queue(), vec![task2, task3, task1]);
    }

    #[test]
    fn jobs_to_activate_respects_concurrency() {
        let sched = Scheduler::new(2);
        let reg = JobRegistry::new();
        let index = NativeTaskIndex::default();

        let j1 = make_job("https://example.test/1");
        let j2 = make_job("https://example.test/2");
        let j3 = make_job("https://example.test/3");
        let g1 = j1.gid;
        let g2 = j2.gid;
        let task1 = j1.task_id.clone();
        let task2 = j2.task_id.clone();
        let task3 = j3.task_id.clone();
        reg.insert(j1).unwrap();
        reg.insert(j2).unwrap();
        reg.insert(j3).unwrap();
        sched.enqueue_task(task1);
        sched.enqueue_task(task2);
        sched.enqueue_task(task3);

        let to_activate = sched.jobs_to_activate(&reg, &index);
        assert_eq!(to_activate.len(), 2);
        assert_eq!(to_activate, vec![g1, g2]);
    }

    #[test]
    fn jobs_to_activate_with_existing_active() {
        let sched = Scheduler::new(2);
        let reg = JobRegistry::new();
        let index = NativeTaskIndex::default();

        let mut active_job = make_job("a");
        active_job.status = Status::Active;
        reg.insert(active_job).unwrap();

        let j1 = make_job("https://example.test/1");
        let j2 = make_job("https://example.test/2");
        let g1 = j1.gid;
        let task1 = j1.task_id.clone();
        let task2 = j2.task_id.clone();
        reg.insert(j1).unwrap();
        reg.insert(j2).unwrap();
        sched.enqueue_task(task1);
        sched.enqueue_task(task2);

        let to_activate = sched.jobs_to_activate(&reg, &index);
        assert_eq!(to_activate.len(), 1);
        assert_eq!(to_activate[0], g1);
    }

    #[test]
    fn jobs_to_activate_at_capacity_returns_empty() {
        let sched = Scheduler::new(1);
        let reg = JobRegistry::new();
        let index = NativeTaskIndex::default();

        let mut active_job = make_job("a");
        active_job.status = Status::Active;
        reg.insert(active_job).unwrap();

        sched.enqueue_task(TaskId::new());

        let to_activate = sched.jobs_to_activate(&reg, &index);
        assert!(to_activate.is_empty());
    }

    #[test]
    fn native_tasks_to_activate_does_not_count_seeding_tasks_as_download_slots() {
        let sched = Scheduler::new(1);
        let reg = JobRegistry::new();

        let mut seeding_job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:feedface".into()],
            PathBuf::from("/tmp/seed"),
        );
        seeding_job.status = Status::Seeding;
        reg.insert(seeding_job).unwrap();

        let waiting = make_job("https://example.test/next.bin");
        let waiting_task_id = waiting.task_id.clone();
        reg.insert(waiting).unwrap();
        sched.enqueue_task(waiting_task_id.clone());

        let to_activate = sched.native_tasks_to_activate(&reg);

        assert_eq!(to_activate, vec![waiting_task_id]);
    }

    #[test]
    fn native_tasks_to_activate_returns_task_ids_without_stale_queue_entries() {
        let sched = Scheduler::new(3);
        let reg = JobRegistry::new();
        let job = make_job("https://example.test/file.bin");
        let task_id = job.task_id.clone();

        reg.insert(job).unwrap();
        sched.enqueue_task(TaskId::new());
        sched.enqueue_task(task_id.clone());

        let to_activate = sched.native_tasks_to_activate(&reg);

        assert_eq!(to_activate, vec![task_id]);
    }

    #[test]
    fn set_max_concurrent_updates() {
        let sched = Scheduler::new(5);
        sched.set_max_concurrent(10);
        assert_eq!(sched.max_concurrent(), 10);

        // Setting 0 clamps to 1.
        sched.set_max_concurrent(0);
        assert_eq!(sched.max_concurrent(), 1);
    }
}
