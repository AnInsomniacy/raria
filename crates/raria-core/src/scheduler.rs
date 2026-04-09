// raria-core: Scheduler — manages job queue ordering and concurrency.
//
// The scheduler controls which jobs are active, how many run concurrently,
// and handles the waiting → active state transitions.

use crate::job::{Gid, Status};
use crate::registry::JobRegistry;
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

/// Controls the execution queue for download jobs.
#[derive(Debug, Clone)]
pub struct Scheduler {
    /// Maximum number of concurrently active jobs.
    max_concurrent: u32,
    /// Ordered queue of waiting GIDs. Front = next to activate.
    queue: Arc<RwLock<VecDeque<Gid>>>,
}

impl Scheduler {
    /// Create a new scheduler with the given concurrency limit.
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            max_concurrent: max_concurrent.max(1),
            queue: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    /// Enqueue a job GID at the back of the waiting queue.
    pub fn enqueue(&self, gid: Gid) {
        let mut queue = self.queue.write().unwrap();
        queue.push_back(gid);
    }

    /// Enqueue a job GID at a specific position (0-indexed).
    /// If `position` exceeds the queue length, it is appended to the end.
    pub fn enqueue_at(&self, gid: Gid, position: usize) {
        let mut queue = self.queue.write().unwrap();
        let pos = position.min(queue.len());
        queue.insert(pos, gid);
    }

    /// Remove a GID from the waiting queue.
    pub fn dequeue(&self, gid: Gid) -> bool {
        let mut queue = self.queue.write().unwrap();
        if let Some(pos) = queue.iter().position(|g| *g == gid) {
            queue.remove(pos);
            true
        } else {
            false
        }
    }

    /// Return the current queue (in order).
    pub fn waiting_queue(&self) -> Vec<Gid> {
        let queue = self.queue.read().unwrap();
        queue.iter().copied().collect()
    }

    /// The number of jobs in the waiting queue.
    pub fn queue_len(&self) -> usize {
        let queue = self.queue.read().unwrap();
        queue.len()
    }

    /// Move a GID to a different position in the queue.
    ///
    /// Supports aria2-compatible position semantics:
    /// - `PositionHow::Set`: absolute position from beginning
    /// - `PositionHow::Cur`: relative to current position
    /// - `PositionHow::End`: position from end
    ///
    /// Returns the new position index, or error if GID not found.
    pub fn change_position(
        &self,
        gid: Gid,
        pos: i32,
        how: crate::engine::PositionHow,
    ) -> anyhow::Result<usize> {
        use crate::engine::PositionHow;
        let mut queue = self.queue.write().unwrap();
        let cur_pos = queue
            .iter()
            .position(|g| *g == gid)
            .ok_or_else(|| anyhow::anyhow!("GID {gid} not in queue"))?;
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
        queue.insert(new_pos, gid);
        Ok(new_pos)
    }

    /// Determine which GIDs should be promoted from Waiting → Active.
    ///
    /// Checks the registry for the count of currently Active jobs,
    /// and returns GIDs from the front of the queue that can be activated.
    pub fn jobs_to_activate(&self, registry: &JobRegistry) -> Vec<Gid> {
        let active_count = registry.by_status(Status::Active).len() as u32;
        if active_count >= self.max_concurrent {
            return Vec::new();
        }

        let slots = (self.max_concurrent - active_count) as usize;
        let queue = self.queue.read().unwrap();
        queue.iter().take(slots).copied().collect()
    }

    /// The maximum number of concurrent downloads.
    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent
    }

    /// Update the maximum concurrency.
    pub fn set_max_concurrent(&mut self, max: u32) {
        self.max_concurrent = max.max(1);
    }
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
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);

        sched.enqueue(g1);
        sched.enqueue(g2);

        let queue = sched.waiting_queue();
        assert_eq!(queue, vec![g1, g2]);
    }

    #[test]
    fn enqueue_at_inserts_at_position() {
        let sched = Scheduler::new(5);
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);
        let g3 = Gid::from_raw(3);

        sched.enqueue(g1);
        sched.enqueue(g3);
        sched.enqueue_at(g2, 1); // insert between g1 and g3

        let queue = sched.waiting_queue();
        assert_eq!(queue, vec![g1, g2, g3]);
    }

    #[test]
    fn enqueue_at_beyond_length_appends() {
        let sched = Scheduler::new(5);
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);

        sched.enqueue(g1);
        sched.enqueue_at(g2, 100);

        let queue = sched.waiting_queue();
        assert_eq!(queue, vec![g1, g2]);
    }

    #[test]
    fn dequeue_removes_gid() {
        let sched = Scheduler::new(5);
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);
        sched.enqueue(g1);
        sched.enqueue(g2);

        assert!(sched.dequeue(g1));
        assert_eq!(sched.waiting_queue(), vec![g2]);
    }

    #[test]
    fn dequeue_nonexistent_returns_false() {
        let sched = Scheduler::new(5);
        assert!(!sched.dequeue(Gid::from_raw(99)));
    }

    #[test]
    fn change_position_moves_gid() {
        let sched = Scheduler::new(5);
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);
        let g3 = Gid::from_raw(3);
        sched.enqueue(g1);
        sched.enqueue(g2);
        sched.enqueue(g3);

        // Move g3 to front (POS_SET=0).
        use crate::engine::PositionHow;
        let new_pos = sched.change_position(g3, 0, PositionHow::Set).unwrap();
        assert_eq!(new_pos, 0);
        assert_eq!(sched.waiting_queue(), vec![g3, g1, g2]);
    }

    #[test]
    fn change_position_nonexistent_returns_error() {
        let sched = Scheduler::new(5);
        use crate::engine::PositionHow;
        let result = sched.change_position(Gid::from_raw(99), 0, PositionHow::Set);
        assert!(result.is_err());
    }

    #[test]
    fn change_position_cur_moves_relative() {
        let sched = Scheduler::new(5);
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);
        let g3 = Gid::from_raw(3);
        sched.enqueue(g1);
        sched.enqueue(g2);
        sched.enqueue(g3);

        // g1 is at pos 0; move +1 relative → pos 1.
        use crate::engine::PositionHow;
        let new_pos = sched.change_position(g1, 1, PositionHow::Cur).unwrap();
        assert_eq!(new_pos, 1);
        assert_eq!(sched.waiting_queue(), vec![g2, g1, g3]);
    }

    #[test]
    fn change_position_end_moves_from_tail() {
        let sched = Scheduler::new(5);
        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);
        let g3 = Gid::from_raw(3);
        sched.enqueue(g1);
        sched.enqueue(g2);
        sched.enqueue(g3);

        // Move g1 to end (POS_END=0 → len).
        use crate::engine::PositionHow;
        let new_pos = sched.change_position(g1, 0, PositionHow::End).unwrap();
        assert_eq!(new_pos, 2);
        assert_eq!(sched.waiting_queue(), vec![g2, g3, g1]);
    }

    #[test]
    fn jobs_to_activate_respects_concurrency() {
        let sched = Scheduler::new(2);
        let reg = JobRegistry::new();

        let g1 = Gid::from_raw(1);
        let g2 = Gid::from_raw(2);
        let g3 = Gid::from_raw(3);
        sched.enqueue(g1);
        sched.enqueue(g2);
        sched.enqueue(g3);

        // No active jobs → should activate first 2.
        let to_activate = sched.jobs_to_activate(&reg);
        assert_eq!(to_activate.len(), 2);
        assert_eq!(to_activate, vec![g1, g2]);
    }

    #[test]
    fn jobs_to_activate_with_existing_active() {
        let sched = Scheduler::new(2);
        let reg = JobRegistry::new();

        // Add an active job to the registry.
        let mut active_job = make_job("a");
        active_job.status = Status::Active;
        reg.insert(active_job).unwrap();

        let g1 = Gid::from_raw(100);
        let g2 = Gid::from_raw(200);
        sched.enqueue(g1);
        sched.enqueue(g2);

        // 1 active → only 1 more slot available.
        let to_activate = sched.jobs_to_activate(&reg);
        assert_eq!(to_activate.len(), 1);
        assert_eq!(to_activate[0], g1);
    }

    #[test]
    fn jobs_to_activate_at_capacity_returns_empty() {
        let sched = Scheduler::new(1);
        let reg = JobRegistry::new();

        let mut active_job = make_job("a");
        active_job.status = Status::Active;
        reg.insert(active_job).unwrap();

        sched.enqueue(Gid::from_raw(100));

        let to_activate = sched.jobs_to_activate(&reg);
        assert!(to_activate.is_empty());
    }

    #[test]
    fn set_max_concurrent_updates() {
        let mut sched = Scheduler::new(5);
        sched.set_max_concurrent(10);
        assert_eq!(sched.max_concurrent(), 10);

        // Setting 0 clamps to 1.
        sched.set_max_concurrent(0);
        assert_eq!(sched.max_concurrent(), 1);
    }
}
