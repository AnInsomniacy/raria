// raria-core: Rate limiter using governor.
//
// Provides global and per-job rate limiting for download/upload throughput.

use governor::{Quota, RateLimiter as GovRateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;

/// A throughput rate limiter.
///
/// Wraps governor's rate limiter to provide a simple bytes-per-second API.
/// A limit of 0 means unlimited (no limiter is created).
#[derive(Debug, Clone)]
pub struct RateLimiter {
    limiter: Option<Arc<GovRateLimiter<governor::state::NotKeyed, governor::state::InMemoryState, governor::clock::DefaultClock>>>,
    limit_bps: u64,
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
}
