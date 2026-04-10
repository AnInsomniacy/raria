// raria-core: Download speed tracker.
//
// Implements a sliding-window download speed estimator that calculates
// accurate real-time speed measurements. Used by both individual job
// status reporting and global statistics.
//
// Design: Exponentially weighted moving average (EWMA) with configurable
// smoothing factor. This approach:
// - Uses O(1) memory and O(1) per-update computation
// - Smooths out jitter from bursty network traffic
// - Converges quickly to the actual speed
// - Is used by aria2's speed calculation as well

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Exponentially weighted moving average speed tracker.
///
/// Thread-safe via atomics. Can be shared across concurrent segment tasks
/// to aggregate speed.
pub struct SpeedTracker {
    /// Total bytes recorded since creation.
    total_bytes: AtomicU64,
    /// Current EWMA speed estimate in bytes/sec, stored as integer.
    current_speed: AtomicU64,
    /// Timestamp of the last sample, stored as nanos since an epoch.
    last_sample_nanos: AtomicU64,
    /// Smoothing factor * 1000 (to avoid floats in atomics).
    /// Default 200 = 0.2 — responsive but smooth.
    alpha_x1000: u32,
    /// Creation time — used for overall average calculation.
    created_at: Instant,
}

impl SpeedTracker {
    /// Create a new speed tracker with default smoothing (α = 0.2).
    pub fn new() -> Self {
        Self::with_alpha(200)
    }

    /// Create a speed tracker with custom smoothing factor.
    ///
    /// `alpha_x1000`: smoothing factor × 1000.
    /// - 200 (0.2) = responsive, good for real-time display
    /// - 100 (0.1) = smoother, less jitter
    /// - 500 (0.5) = very responsive, more jitter
    pub fn with_alpha(alpha_x1000: u32) -> Self {
        Self {
            total_bytes: AtomicU64::new(0),
            current_speed: AtomicU64::new(0),
            last_sample_nanos: AtomicU64::new(0),
            alpha_x1000,
            created_at: Instant::now(),
        }
    }

    /// Record bytes downloaded. Call this from the download loop.
    ///
    /// `bytes`: number of bytes just downloaded in this chunk.
    pub fn record(&self, bytes: u64) {
        self.total_bytes.fetch_add(bytes, Ordering::Relaxed);

        let now = Instant::now();
        let now_nanos = (now.duration_since(self.created_at).as_nanos() as u64).max(1);
        let prev_nanos = self.last_sample_nanos.swap(now_nanos, Ordering::Relaxed);

        if prev_nanos == 0 {
            // First sample — can't compute speed yet.
            return;
        }

        let elapsed_nanos = now_nanos.saturating_sub(prev_nanos);
        if elapsed_nanos == 0 {
            return;
        }

        // Instantaneous speed = bytes * 1_000_000_000 / elapsed_nanos.
        let instant_speed = bytes.saturating_mul(1_000_000_000) / elapsed_nanos;

        // EWMA update: speed = α * instant_speed + (1 - α) * prev_speed.
        let prev_speed = self.current_speed.load(Ordering::Relaxed);
        let alpha = self.alpha_x1000 as u64;
        let new_speed = if prev_speed == 0 {
            instant_speed.max(1)
        } else {
            (alpha * instant_speed + (1000 - alpha) * prev_speed) / 1000
        };

        self.current_speed.store(new_speed, Ordering::Relaxed);
    }

    /// Get the current EWMA speed estimate in bytes/sec.
    pub fn speed_bps(&self) -> u64 {
        self.current_speed.load(Ordering::Relaxed)
    }

    /// Get the total bytes recorded.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    /// Get the overall average speed (total_bytes / elapsed_time).
    pub fn average_speed_bps(&self) -> u64 {
        let elapsed = self.created_at.elapsed();
        let secs = elapsed.as_secs_f64();
        if secs < 0.001 {
            return 0;
        }
        (self.total_bytes.load(Ordering::Relaxed) as f64 / secs) as u64
    }

    /// Reset the tracker.
    pub fn reset(&self) {
        self.total_bytes.store(0, Ordering::Relaxed);
        self.current_speed.store(0, Ordering::Relaxed);
        self.last_sample_nanos.store(0, Ordering::Relaxed);
    }
}

impl Default for SpeedTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn new_tracker_starts_at_zero() {
        let tracker = SpeedTracker::new();
        assert_eq!(tracker.speed_bps(), 0);
        assert_eq!(tracker.total_bytes(), 0);
    }

    #[test]
    fn records_total_bytes() {
        let tracker = SpeedTracker::new();
        tracker.record(1000);
        tracker.record(2000);
        assert_eq!(tracker.total_bytes(), 3000);
    }

    #[test]
    fn first_record_does_not_produce_speed() {
        let tracker = SpeedTracker::new();
        tracker.record(1000);
        assert_eq!(tracker.speed_bps(), 0, "first sample can't compute speed");
    }

    #[test]
    fn subsequent_records_produce_speed() {
        let tracker = SpeedTracker::new();
        tracker.record(1000);
        // Small sleep to ensure elapsed time > 0.
        thread::sleep(Duration::from_millis(10));
        tracker.record(1000);
        assert!(
            tracker.speed_bps() > 0,
            "speed should be positive after two samples"
        );
    }

    #[test]
    fn speed_is_reasonable() {
        let tracker = SpeedTracker::new();
        // Simulate 100KB every 100ms = ~1MB/s.
        for _ in 0..10 {
            tracker.record(100_000);
            thread::sleep(Duration::from_millis(100));
        }
        let speed = tracker.speed_bps();
        // Allow generous tolerance: 100KB/s to 10MB/s.
        assert!(
            speed > 50_000 && speed < 10_000_000,
            "speed {speed} B/s out of expected range (100KB/s - 10MB/s)"
        );
    }

    #[test]
    fn reset_clears_everything() {
        let tracker = SpeedTracker::new();
        tracker.record(5000);
        thread::sleep(Duration::from_millis(10));
        tracker.record(5000);

        tracker.reset();
        assert_eq!(tracker.speed_bps(), 0);
        assert_eq!(tracker.total_bytes(), 0);
    }

    #[test]
    fn average_speed_computes_correctly() {
        let tracker = SpeedTracker::new();
        tracker.record(1_000_000);
        thread::sleep(Duration::from_millis(100));
        let avg = tracker.average_speed_bps();
        // 1MB over ~100ms should be ~10MB/s. Allow wide tolerance.
        assert!(
            avg > 100_000,
            "average speed {avg} too low for 1MB in ~100ms"
        );
    }

    #[test]
    fn concurrent_recording_is_safe() {
        let tracker = std::sync::Arc::new(SpeedTracker::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let t = std::sync::Arc::clone(&tracker);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    t.record(1000);
                    thread::sleep(Duration::from_micros(100));
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(tracker.total_bytes(), 400_000);
    }

    #[test]
    fn custom_alpha_works() {
        let tracker = SpeedTracker::with_alpha(500); // Very responsive.
        tracker.record(1000);
        thread::sleep(Duration::from_millis(10));
        tracker.record(1000);
        // Should still produce a positive speed.
        assert!(tracker.speed_bps() > 0);
    }
}
