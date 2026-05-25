// raria-core: CancellationToken management.
//
// Provides a registry of per-task cancellation tokens so that transfers can be
// gracefully cancelled from the scheduler, native API, or CLI.

use crate::native::TaskId;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Manages cancellation tokens keyed by native task id.
#[derive(Debug, Clone)]
pub struct CancelRegistry {
    inner: Arc<RwLock<HashMap<TaskId, CancellationToken>>>,
}

impl CancelRegistry {
    /// Create a new empty cancel registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create and register a new cancellation token for a task.
    ///
    /// Returns the token. If a token already exists for this task id, it is
    /// replaced (the old token is NOT cancelled).
    pub fn register(&self, task_id: TaskId) -> CancellationToken {
        let token = CancellationToken::new();
        let mut inner = self.inner.write();
        inner.insert(task_id, token.clone());
        token
    }

    /// Create a child token linked to the task's token.
    ///
    /// Returns `None` if no token is registered for this task id.
    pub fn child_token(&self, task_id: &TaskId) -> Option<CancellationToken> {
        let inner = self.inner.read();
        inner.get(task_id).map(|t| t.child_token())
    }

    /// Cancel a task's token.
    ///
    /// Returns `true` if the token existed and was cancelled.
    pub fn cancel(&self, task_id: &TaskId) -> bool {
        let inner = self.inner.read();
        if let Some(token) = inner.get(task_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Cancel every registered task token.
    pub fn cancel_all(&self) {
        let inner = self.inner.read();
        for token in inner.values() {
            token.cancel();
        }
    }

    /// Check if a task's token has been cancelled.
    pub fn is_cancelled(&self, task_id: &TaskId) -> Option<bool> {
        let inner = self.inner.read();
        inner.get(task_id).map(|t| t.is_cancelled())
    }

    /// Remove a token from the registry (e.g., after job completion).
    pub fn remove(&self, task_id: &TaskId) -> Option<CancellationToken> {
        let mut inner = self.inner.write();
        inner.remove(task_id)
    }

    /// Number of registered tokens.
    pub fn len(&self) -> usize {
        let inner = self.inner.read();
        inner.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for CancelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let reg = CancelRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_creates_token() {
        let reg = CancelRegistry::new();
        let task_id = TaskId::new();
        let token = reg.register(task_id);
        assert!(!token.is_cancelled());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn cancel_cancels_token() {
        let reg = CancelRegistry::new();
        let task_id = TaskId::new();
        let token = reg.register(task_id.clone());

        assert!(reg.cancel(&task_id));
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_all_cancels_every_registered_token() {
        let reg = CancelRegistry::new();
        let first = reg.register(TaskId::new());
        let second = reg.register(TaskId::new());

        reg.cancel_all();

        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let reg = CancelRegistry::new();
        assert!(!reg.cancel(&TaskId::new()));
    }

    #[test]
    fn is_cancelled_returns_correct_state() {
        let reg = CancelRegistry::new();
        let task_id = TaskId::new();
        reg.register(task_id.clone());

        assert_eq!(reg.is_cancelled(&task_id), Some(false));
        reg.cancel(&task_id);
        assert_eq!(reg.is_cancelled(&task_id), Some(true));
    }

    #[test]
    fn is_cancelled_nonexistent_returns_none() {
        let reg = CancelRegistry::new();
        assert!(reg.is_cancelled(&TaskId::new()).is_none());
    }

    #[test]
    fn child_token_is_cancelled_when_parent_is() {
        let reg = CancelRegistry::new();
        let task_id = TaskId::new();
        reg.register(task_id.clone());

        let child = reg.child_token(&task_id).expect("token exists");
        assert!(!child.is_cancelled());

        reg.cancel(&task_id);
        assert!(child.is_cancelled());
    }

    #[test]
    fn child_token_nonexistent_returns_none() {
        let reg = CancelRegistry::new();
        assert!(reg.child_token(&TaskId::new()).is_none());
    }

    #[test]
    fn remove_cleans_up() {
        let reg = CancelRegistry::new();
        let task_id = TaskId::new();
        reg.register(task_id.clone());

        let removed = reg.remove(&task_id);
        assert!(removed.is_some());
        assert!(reg.is_empty());
        assert!(reg.is_cancelled(&task_id).is_none());
    }

    #[test]
    fn register_replaces_existing() {
        let reg = CancelRegistry::new();
        let task_id = TaskId::new();
        let token1 = reg.register(task_id.clone());
        let token2 = reg.register(task_id.clone());

        // token1 is now orphaned but not cancelled.
        assert!(!token1.is_cancelled());
        // token2 is the new active token.
        reg.cancel(&task_id);
        assert!(token2.is_cancelled());
        // token1 is NOT cancelled (it was replaced, not cancelled).
        assert!(!token1.is_cancelled());
    }
}
