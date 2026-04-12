// raria-core: CancellationToken management.
//
// Provides a registry of per-job cancellation tokens so that any job
// can be gracefully cancelled from the scheduler, RPC layer, or CLI.

use crate::job::Gid;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Manages cancellation tokens keyed by job GID.
#[derive(Debug, Clone)]
pub struct CancelRegistry {
    inner: Arc<RwLock<HashMap<Gid, CancellationToken>>>,
}

impl CancelRegistry {
    /// Create a new empty cancel registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create and register a new cancellation token for a job.
    ///
    /// Returns the token. If a token already exists for this GID, it is
    /// replaced (the old token is NOT cancelled).
    pub fn register(&self, gid: Gid) -> CancellationToken {
        let token = CancellationToken::new();
        let mut inner = self.inner.write();
        inner.insert(gid, token.clone());
        token
    }

    /// Create a child token linked to the job's token.
    ///
    /// Returns `None` if no token is registered for this GID.
    pub fn child_token(&self, gid: Gid) -> Option<CancellationToken> {
        let inner = self.inner.read();
        inner.get(&gid).map(|t| t.child_token())
    }

    /// Cancel a job's token.
    ///
    /// Returns `true` if the token existed and was cancelled.
    pub fn cancel(&self, gid: Gid) -> bool {
        let inner = self.inner.read();
        if let Some(token) = inner.get(&gid) {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Cancel every registered job token.
    pub fn cancel_all(&self) {
        let inner = self.inner.read();
        for token in inner.values() {
            token.cancel();
        }
    }

    /// Check if a job's token has been cancelled.
    pub fn is_cancelled(&self, gid: Gid) -> Option<bool> {
        let inner = self.inner.read();
        inner.get(&gid).map(|t| t.is_cancelled())
    }

    /// Remove a token from the registry (e.g., after job completion).
    pub fn remove(&self, gid: Gid) -> Option<CancellationToken> {
        let mut inner = self.inner.write();
        inner.remove(&gid)
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
    use crate::job::Gid;

    #[test]
    fn new_registry_is_empty() {
        let reg = CancelRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_creates_token() {
        let reg = CancelRegistry::new();
        let gid = Gid::from_raw(1);
        let token = reg.register(gid);
        assert!(!token.is_cancelled());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn cancel_cancels_token() {
        let reg = CancelRegistry::new();
        let gid = Gid::from_raw(1);
        let token = reg.register(gid);

        assert!(reg.cancel(gid));
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_all_cancels_every_registered_token() {
        let reg = CancelRegistry::new();
        let first = reg.register(Gid::from_raw(1));
        let second = reg.register(Gid::from_raw(2));

        reg.cancel_all();

        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let reg = CancelRegistry::new();
        assert!(!reg.cancel(Gid::from_raw(99)));
    }

    #[test]
    fn is_cancelled_returns_correct_state() {
        let reg = CancelRegistry::new();
        let gid = Gid::from_raw(1);
        reg.register(gid);

        assert_eq!(reg.is_cancelled(gid), Some(false));
        reg.cancel(gid);
        assert_eq!(reg.is_cancelled(gid), Some(true));
    }

    #[test]
    fn is_cancelled_nonexistent_returns_none() {
        let reg = CancelRegistry::new();
        assert!(reg.is_cancelled(Gid::from_raw(99)).is_none());
    }

    #[test]
    fn child_token_is_cancelled_when_parent_is() {
        let reg = CancelRegistry::new();
        let gid = Gid::from_raw(1);
        reg.register(gid);

        let child = reg.child_token(gid).expect("token exists");
        assert!(!child.is_cancelled());

        reg.cancel(gid);
        assert!(child.is_cancelled());
    }

    #[test]
    fn child_token_nonexistent_returns_none() {
        let reg = CancelRegistry::new();
        assert!(reg.child_token(Gid::from_raw(99)).is_none());
    }

    #[test]
    fn remove_cleans_up() {
        let reg = CancelRegistry::new();
        let gid = Gid::from_raw(1);
        reg.register(gid);

        let removed = reg.remove(gid);
        assert!(removed.is_some());
        assert!(reg.is_empty());
        assert!(reg.is_cancelled(gid).is_none());
    }

    #[test]
    fn register_replaces_existing() {
        let reg = CancelRegistry::new();
        let gid = Gid::from_raw(1);
        let token1 = reg.register(gid);
        let token2 = reg.register(gid);

        // token1 is now orphaned but not cancelled.
        assert!(!token1.is_cancelled());
        // token2 is the new active token.
        reg.cancel(gid);
        assert!(token2.is_cancelled());
        // token1 is NOT cancelled (it was replaced, not cancelled).
        assert!(!token1.is_cancelled());
    }
}
