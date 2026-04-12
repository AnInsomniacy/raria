// raria-core: Rate limiter using governor.
//
// Provides global and per-job rate limiting for download/upload throughput.

use arc_swap::ArcSwapOption;
use governor::{Quota, RateLimiter as GovRateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// A throughput rate limiter.
///
/// Wraps governor's rate limiter to provide a simple bytes-per-second API.
/// A limit of 0 means unlimited (no limiter is created).
#[derive(Debug, Clone)]
pub struct RateLimiter {
    limiter: Option<
        Arc<
            GovRateLimiter<
                governor::state::NotKeyed,
                governor::state::InMemoryState,
                governor::clock::DefaultClock,
            >,
        >,
    >,
    limit_bps: u64,
}

/// A read-mostly limiter handle whose active limiter can be swapped at runtime.
#[derive(Debug, Default)]
pub struct SharedRateLimiter {
    inner: ArcSwapOption<RateLimiter>,
    chained: Option<Arc<SharedRateLimiter>>,
}

impl RateLimiter {
    /// Create a rate limiter with the given bytes-per-second limit.
    ///
    /// - `limit_bps = 0`: unlimited (no throttling).
    /// - `limit_bps > 0`: throttle to at most `limit_bps` bytes/sec.
    pub fn new(limit_bps: u64) -> Self {
        let limiter = if limit_bps > 0 {
            // We use the limit as the burst capacity and refill rate.
            // Each "cell" consumed = 1 byte.
            // To avoid overflow, cap at u32::MAX.
            let cells = NonZeroU32::new(limit_bps.min(u32::MAX as u64) as u32).unwrap();
            let quota = Quota::per_second(cells);
            Some(Arc::new(GovRateLimiter::direct(quota)))
        } else {
            None
        };

        Self { limiter, limit_bps }
    }

    /// Create an unlimited rate limiter (no throttling).
    pub fn unlimited() -> Self {
        Self::new(0)
    }

    /// The configured limit in bytes per second (0 = unlimited).
    pub fn limit_bps(&self) -> u64 {
        self.limit_bps
    }

    /// Whether this limiter is actively rate-limiting.
    pub fn is_limited(&self) -> bool {
        self.limiter.is_some()
    }

    /// Consume `n` bytes worth of quota. If the limiter is unlimited,
    /// this returns immediately. Otherwise it waits until quota is available.
    ///
    /// For efficiency, `n` is clamped to the burst size. Large writes
    /// should call this in a loop with chunk sizes.
    pub async fn consume(&self, n: u32) {
        if let Some(ref limiter) = self.limiter {
            if n == 0 {
                return;
            }
            // governor's `until_n_cells_ready` wants a NonZeroU32.
            let cells = NonZeroU32::new(n.max(1)).unwrap();
            // Ignore the error variant (InsufficientCapacity) by clamping.
            match limiter.until_n_ready(cells).await {
                Ok(()) => {}
                Err(_) => {
                    // Requested more than burst capacity; just wait for 1 cell
                    // repeatedly. This handles the edge case gracefully.
                    for _ in 0..n {
                        limiter.until_ready().await;
                    }
                }
            }
        }
    }
}

impl SharedRateLimiter {
    /// Create a new shared limiter. Pass `0` to disable rate limiting.
    pub fn new(limit_bps: u64) -> Self {
        let inner = if limit_bps > 0 {
            Some(Arc::new(RateLimiter::new(limit_bps)))
        } else {
            None
        };
        Self {
            inner: ArcSwapOption::from(inner),
            chained: None,
        }
    }

    /// Create a limiter that enforces its own limit and an upstream shared limit.
    pub fn chained(limit_bps: u64, chained: Arc<SharedRateLimiter>) -> Self {
        let inner = if limit_bps > 0 {
            Some(Arc::new(RateLimiter::new(limit_bps)))
        } else {
            None
        };
        Self {
            inner: ArcSwapOption::from(inner),
            chained: Some(chained),
        }
    }

    /// Current limit in bytes per second, or `0` if unlimited.
    pub fn limit_bps(&self) -> u64 {
        let own = self
            .inner
            .load_full()
            .map(|limiter| limiter.limit_bps())
            .unwrap_or(0);
        let chained = self
            .chained
            .as_ref()
            .map(|limiter| limiter.limit_bps())
            .unwrap_or(0);
        match (own, chained) {
            (0, 0) => 0,
            (0, other) => other,
            (own, 0) => own,
            (own, chained) => own.min(chained),
        }
    }

    /// Returns `true` if a rate limit is currently active.
    pub fn is_limited(&self) -> bool {
        self.inner.load().is_some()
    }

    /// Hot-swap the rate limit. Pass `0` to disable. Takes effect immediately.
    pub fn update_limit(&self, limit_bps: u64) {
        let next = if limit_bps > 0 {
            Some(Arc::new(RateLimiter::new(limit_bps)))
        } else {
            None
        };
        self.inner.store(next);
    }

    /// Consume `n` bytes of bandwidth, blocking until the limiter allows it.
    pub async fn consume(&self, n: u32) {
        let mut remaining = n;
        while remaining > 0 {
            let step = remaining.min(16 * 1024);
            let mut current = Some(self);
            while let Some(limiter) = current {
                if let Some(inner) = limiter.inner.load_full() {
                    inner.consume(step).await;
                }
                current = limiter.chained.as_deref();
            }
            remaining -= step;
        }
    }

    /// Consume bytes while allowing cancellation to abort a blocked wait.
    ///
    /// Returns `true` if all requested quota was consumed, or `false` if
    /// cancellation fired before the limiter finished waiting.
    pub async fn consume_cancellable(&self, n: u32, cancel: &CancellationToken) -> bool {
        let mut remaining = n;
        while remaining > 0 {
            let step = remaining.min(16 * 1024);
            let mut current = Some(self);
            while let Some(limiter) = current {
                if let Some(inner) = limiter.inner.load_full() {
                    tokio::select! {
                        _ = inner.consume(step) => {}
                        _ = cancel.cancelled() => return false,
                    }
                }
                current = limiter.chained.as_deref();
            }
            remaining -= step;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_limiter_is_not_limited() {
        let limiter = RateLimiter::unlimited();
        assert!(!limiter.is_limited());
        assert_eq!(limiter.limit_bps(), 0);
    }

    #[test]
    fn limited_limiter_reports_limit() {
        let limiter = RateLimiter::new(1024);
        assert!(limiter.is_limited());
        assert_eq!(limiter.limit_bps(), 1024);
    }

    #[test]
    fn zero_limit_is_unlimited() {
        let limiter = RateLimiter::new(0);
        assert!(!limiter.is_limited());
    }

    #[tokio::test]
    async fn unlimited_consume_returns_immediately() {
        let limiter = RateLimiter::unlimited();
        // Should complete instantly.
        limiter.consume(1000).await;
    }

    #[tokio::test]
    async fn limited_consume_zero_returns_immediately() {
        let limiter = RateLimiter::new(100);
        limiter.consume(0).await;
    }

    #[tokio::test]
    async fn limited_consume_within_burst_succeeds() {
        // 10000 bytes/sec burst, consuming 100 bytes should be instant.
        let limiter = RateLimiter::new(10000);
        limiter.consume(100).await;
    }

    #[test]
    fn shared_rate_limiter_defaults_to_unlimited() {
        let limiter = SharedRateLimiter::new(0);
        assert_eq!(limiter.limit_bps(), 0);
        assert!(!limiter.is_limited());
    }

    #[test]
    fn shared_rate_limiter_can_swap_limits() {
        let limiter = SharedRateLimiter::new(1024);
        assert_eq!(limiter.limit_bps(), 1024);
        assert!(limiter.is_limited());

        limiter.update_limit(2048);
        assert_eq!(limiter.limit_bps(), 2048);
        assert!(limiter.is_limited());

        limiter.update_limit(0);
        assert_eq!(limiter.limit_bps(), 0);
        assert!(!limiter.is_limited());
    }

    #[test]
    fn chained_shared_rate_limiter_reports_the_tightest_limit() {
        let global = Arc::new(SharedRateLimiter::new(2048));
        let limiter = SharedRateLimiter::chained(1024, Arc::clone(&global));
        assert_eq!(limiter.limit_bps(), 1024);

        limiter.update_limit(4096);
        assert_eq!(limiter.limit_bps(), 2048);
    }

    #[test]
    fn chained_shared_rate_limiter_uses_secondary_limit_when_primary_is_unlimited() {
        let global = Arc::new(SharedRateLimiter::new(2048));
        let limiter = SharedRateLimiter::chained(0, Arc::clone(&global));
        assert_eq!(limiter.limit_bps(), 2048);
    }

    #[tokio::test]
    async fn consume_cancellable_returns_false_when_cancelled_mid_wait() {
        let limiter = SharedRateLimiter::new(1);
        let cancel = CancellationToken::new();
        cancel.cancel();

        assert!(!limiter.consume_cancellable(16 * 1024, &cancel).await);
    }
}
