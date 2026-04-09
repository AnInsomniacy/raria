// raria-range: SegmentExecutor — concurrent multi-segment download orchestrator.
//
// This module is the core download engine. It:
// 1. Takes an Arc<dyn ByteSourceBackend> (clonable across tasks)
// 2. Spawns one tokio task per segment, bounded by a Semaphore
// 3. Each task retries on failure with exponential backoff
// 4. Each task respects CancellationToken
// 5. All tasks write to the same file at their respective offsets
// 6. Progress is reported via an atomic callback
//
// The old executor was sequential. This one is truly concurrent.

use crate::backend::{ByteSourceBackend, OpenContext};
use anyhow::{Context, Result};
use raria_core::limiter::RateLimiter;
use raria_core::segment::{SegmentState, SegmentStatus};
use std::path::Path;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};
use url::Url;

/// Configuration for the segment executor.
#[derive(Clone)]
pub struct ExecutorConfig {
    /// Maximum number of concurrent connections.
    pub max_connections: u32,
    /// Read buffer size in bytes.
    pub buffer_size: usize,
    /// Maximum retries per segment before giving up.
    pub max_retries: u32,
    /// Base delay for exponential backoff on retry (milliseconds).
    pub retry_base_delay_ms: u64,
    /// Optional rate limiter for throttling download speed.
    /// Shared across all concurrent segment tasks.
    pub rate_limiter: Option<Arc<RateLimiter>>,
    /// Optional checkpoint callback. Called periodically with
    /// (segment_id, bytes_downloaded_this_segment) so the engine
    /// can persist segment-level progress to redb.
    pub on_checkpoint: Option<Arc<dyn Fn(u32, u64) + Send + Sync>>,
}

impl std::fmt::Debug for ExecutorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutorConfig")
            .field("max_connections", &self.max_connections)
            .field("buffer_size", &self.buffer_size)
            .field("max_retries", &self.max_retries)
            .field("retry_base_delay_ms", &self.retry_base_delay_ms)
            .field("rate_limiter", &self.rate_limiter.is_some())
            .field("on_checkpoint", &self.on_checkpoint.is_some())
            .finish()
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_connections: 16,
            buffer_size: 64 * 1024, // 64 KiB
            max_retries: 5,
            retry_base_delay_ms: 500,
            rate_limiter: None,
            on_checkpoint: None,
        }
    }
}

/// Per-segment result collected after all tasks complete.
#[derive(Debug, Clone)]
pub struct SegmentResult {
    pub segment_id: u32,
    pub bytes_downloaded: u64,
    pub status: SegmentStatus,
    pub error: Option<String>,
    pub retries_used: u32,
}

/// Orchestrates downloading all segments of a file concurrently.
///
/// Unlike the previous implementation, this executor:
/// - Uses `Arc<dyn ByteSourceBackend>` so each spawned task owns a reference
/// - Uses a `Semaphore` to limit concurrent connections
/// - Retries failed segments with exponential backoff
/// - Reports progress atomically via callback
pub struct SegmentExecutor {
    config: ExecutorConfig,
}

impl SegmentExecutor {
    /// Create a new executor with the given configuration.
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Execute the download of all pending/failed segments **concurrently**.
    ///
    /// Returns a vector of per-segment results. The caller must inspect
    /// these to update its own SegmentState array.
    pub async fn execute(
        &self,
        backend: Arc<dyn ByteSourceBackend>,
        uri: &Url,
        out_path: &Path,
        segments: &[SegmentState],
        cancel: CancellationToken,
        on_progress: Arc<dyn Fn(u32, u64) + Send + Sync>,
    ) -> Result<Vec<SegmentResult>> {
        let semaphore = Arc::new(Semaphore::new(self.config.max_connections as usize));

        // Pre-allocate the output file if we know the total size.
        // Skip pre-allocation for streaming downloads (end == u64::MAX).
        if let Some(last_seg) = segments.last() {
            if last_seg.end > 0 && last_seg.end < u64::MAX {
                let file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(out_path)
                    .await
                    .context("failed to pre-allocate output file")?;
                file.set_len(last_seg.end).await?;
            }
        }

        // Collect work items: (segment_id, resume_offset, remaining_bytes).
        let mut work = Vec::new();
        for (seg_id, seg) in segments.iter().enumerate() {
            if seg.status == SegmentStatus::Done || seg.is_done() {
                continue;
            }
            let remaining = seg.remaining();
            if remaining == 0 {
                continue;
            }
            work.push((seg_id as u32, seg.resume_offset(), remaining));
        }

        // Shared state for collecting results.
        let results: Arc<Mutex<Vec<SegmentResult>>> =
            Arc::new(Mutex::new(Vec::with_capacity(work.len())));

        // Spawn one task per segment.
        let mut handles = Vec::with_capacity(work.len());

        for (seg_id, resume_offset, remaining) in work {
            let backend = Arc::clone(&backend);
            let semaphore = Arc::clone(&semaphore);
            let cancel = cancel.clone();
            let on_progress = Arc::clone(&on_progress);
            let results = Arc::clone(&results);
            let uri = uri.clone();
            let out_path = out_path.to_path_buf();
            let config = self.config.clone();

            let handle = tokio::spawn(async move {
                // Acquire a semaphore permit before starting.
                let _permit = match semaphore.acquire().await {
                    Ok(permit) => permit,
                    Err(_) => {
                        // Semaphore closed — likely shutting down.
                        let mut results = results.lock().await;
                        results.push(SegmentResult {
                            segment_id: seg_id,
                            bytes_downloaded: 0,
                            status: SegmentStatus::Failed,
                            error: Some("semaphore closed".into()),
                            retries_used: 0,
                        });
                        return;
                    }
                };

                let result = Self::download_segment_with_retry(
                    backend.as_ref(),
                    &uri,
                    &out_path,
                    seg_id,
                    resume_offset,
                    remaining,
                    &config,
                    &cancel,
                    on_progress.as_ref(),
                )
                .await;

                let mut results = results.lock().await;
                results.push(result);
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete.
        for handle in handles {
            // We don't propagate JoinError — each task reports its own result.
            let _ = handle.await;
        }

        let results = Arc::try_unwrap(results)
            .expect("all tasks completed, Arc should be unique")
            .into_inner();

        Ok(results)
    }

    /// Download a single segment with retry logic and exponential backoff.
    #[allow(clippy::too_many_arguments)]
    async fn download_segment_with_retry(
        backend: &dyn ByteSourceBackend,
        uri: &Url,
        out_path: &Path,
        seg_id: u32,
        resume_offset: u64,
        remaining: u64,
        config: &ExecutorConfig,
        cancel: &CancellationToken,
        on_progress: &(dyn Fn(u32, u64) + Send + Sync),
    ) -> SegmentResult {
        let mut retries = 0u32;
        let mut total_downloaded = 0u64;
        let mut current_offset = resume_offset;
        let mut current_remaining = remaining;

        loop {
            if cancel.is_cancelled() {
                return SegmentResult {
                    segment_id: seg_id,
                    bytes_downloaded: total_downloaded,
                    status: if total_downloaded >= remaining {
                        SegmentStatus::Done
                    } else {
                        SegmentStatus::Pending
                    },
                    error: None,
                    retries_used: retries,
                };
            }

            match Self::download_segment_once(
                backend,
                uri,
                out_path,
                seg_id,
                current_offset,
                current_remaining,
                config.buffer_size,
                cancel,
                on_progress,
                config.rate_limiter.as_ref().map(|l| l.as_ref()),
                config.on_checkpoint.as_ref().map(|c| c.as_ref()),
            )
            .await
            {
                Ok(bytes) => {
                    total_downloaded += bytes;
                    if total_downloaded >= remaining {
                        return SegmentResult {
                            segment_id: seg_id,
                            bytes_downloaded: total_downloaded,
                            status: SegmentStatus::Done,
                            error: None,
                            retries_used: retries,
                        };
                    }

                    // For streaming segments (unknown size), EOF means done.
                    // The stream ended naturally — that's the whole file.
                    if remaining == u64::MAX && bytes > 0 {
                        return SegmentResult {
                            segment_id: seg_id,
                            bytes_downloaded: total_downloaded,
                            status: SegmentStatus::Done,
                            error: None,
                            retries_used: retries,
                        };
                    }

                    // Partial download (stream ended early). Update offsets for retry.
                    current_offset = resume_offset + total_downloaded;
                    current_remaining = remaining.saturating_sub(total_downloaded);

                    if retries >= config.max_retries {
                        return SegmentResult {
                            segment_id: seg_id,
                            bytes_downloaded: total_downloaded,
                            status: SegmentStatus::Failed,
                            error: Some(format!(
                                "partial download after {retries} retries ({total_downloaded}/{remaining} bytes)"
                            )),
                            retries_used: retries,
                        };
                    }

                    retries += 1;
                    let delay_ms = config.retry_base_delay_ms * 2u64.pow(retries - 1);
                    warn!(
                        seg_id, retries, delay_ms, bytes_so_far = total_downloaded,
                        "segment incomplete, retrying"
                    );

                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(delay_ms)) => {},
                        _ = cancel.cancelled() => {
                            return SegmentResult {
                                segment_id: seg_id,
                                bytes_downloaded: total_downloaded,
                                status: SegmentStatus::Pending,
                                error: None,
                                retries_used: retries,
                            };
                        }
                    }
                }
                Err(e) => {
                    if retries >= config.max_retries {
                        error!(seg_id, retries, error = %e, "segment failed permanently");
                        return SegmentResult {
                            segment_id: seg_id,
                            bytes_downloaded: total_downloaded,
                            status: SegmentStatus::Failed,
                            error: Some(e.to_string()),
                            retries_used: retries,
                        };
                    }

                    retries += 1;
                    let delay_ms = config.retry_base_delay_ms * 2u64.pow(retries - 1);
                    warn!(
                        seg_id, retries, delay_ms, error = %e,
                        "segment error, retrying"
                    );

                    // Update offset for resume after partial failure.
                    current_offset = resume_offset + total_downloaded;
                    current_remaining = remaining - total_downloaded;

                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(delay_ms)) => {},
                        _ = cancel.cancelled() => {
                            return SegmentResult {
                                segment_id: seg_id,
                                bytes_downloaded: total_downloaded,
                                status: SegmentStatus::Pending,
                                error: None,
                                retries_used: retries,
                            };
                        }
                    }
                }
            }
        }
    }

    /// Execute a single attempt at downloading a segment's remaining bytes.
    ///
    /// Writes directly to the file at the correct offset. Returns how many
    /// bytes were read in this attempt.
    #[allow(clippy::too_many_arguments)]
    async fn download_segment_once(
        backend: &dyn ByteSourceBackend,
        uri: &Url,
        out_path: &Path,
        seg_id: u32,
        offset: u64,
        remaining: u64,
        buffer_size: usize,
        cancel: &CancellationToken,
        on_progress: &(dyn Fn(u32, u64) + Send + Sync),
        rate_limiter: Option<&RateLimiter>,
        on_checkpoint: Option<&(dyn Fn(u32, u64) + Send + Sync)>,
    ) -> Result<u64> {
        debug!(seg_id, offset, remaining, "starting segment attempt");

        let ctx = OpenContext::default();
        let mut stream = backend
            .open_from(uri, offset, &ctx)
            .await
            .with_context(|| format!("failed to open stream for segment {seg_id}"))?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(out_path)
            .await
            .with_context(|| format!("failed to open output file for segment {seg_id}"))?;

        file.seek(std::io::SeekFrom::Start(offset)).await?;

        let mut buf = vec![0u8; buffer_size];
        let mut bytes_this_attempt = 0u64;
        let mut bytes_since_checkpoint = 0u64;
        // Checkpoint every 1 MiB to avoid excessive I/O.
        const CHECKPOINT_INTERVAL: u64 = 1024 * 1024;

        loop {
            if cancel.is_cancelled() {
                break;
            }

            let to_read = ((remaining - bytes_this_attempt) as usize).min(buf.len());
            if to_read == 0 {
                break;
            }

            let n = stream.read(&mut buf[..to_read]).await?;
            if n == 0 {
                break; // EOF
            }

            file.write_all(&buf[..n]).await?;
            bytes_this_attempt += n as u64;
            bytes_since_checkpoint += n as u64;

            // Rate limiting: consume bytes from the shared limiter.
            if let Some(limiter) = rate_limiter {
                limiter.consume(n as u32).await;
            }

            on_progress(seg_id, n as u64);

            // Periodic checkpoint for crash recovery.
            if bytes_since_checkpoint >= CHECKPOINT_INTERVAL {
                if let Some(checkpoint) = on_checkpoint {
                    checkpoint(seg_id, bytes_this_attempt);
                }
                bytes_since_checkpoint = 0;
            }
        }

        file.flush().await?;
        debug!(seg_id, bytes = bytes_this_attempt, "segment attempt complete");
        Ok(bytes_this_attempt)
    }
}

/// Convenience function to apply SegmentResults back to SegmentStates.
pub fn apply_results(segments: &mut [SegmentState], results: &[SegmentResult]) {
    for result in results {
        let idx = result.segment_id as usize;
        if idx < segments.len() {
            segments[idx].downloaded += result.bytes_downloaded;
            segments[idx].status = result.status;
        }
    }
}

/// Compute total bytes downloaded from a set of results.
pub fn total_downloaded(results: &[SegmentResult]) -> u64 {
    results.iter().map(|r| r.bytes_downloaded).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ByteStream, FileProbe, ProbeContext};
    use raria_core::segment::{init_segment_states, plan_segments};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

    // ═══════════════════════════════════════════════════════════════════
    // Test Helpers
    // ═══════════════════════════════════════════════════════════════════

    /// A backend that serves data from an in-memory buffer.
    /// Thread-safe and clonable via Arc.
    #[derive(Debug)]
    struct MockBackend {
        data: Vec<u8>,
    }

    #[async_trait::async_trait]
    impl ByteSourceBackend for MockBackend {
        async fn probe(&self, _uri: &Url, _ctx: &ProbeContext) -> Result<FileProbe> {
            Ok(FileProbe {
                size: Some(self.data.len() as u64),
                supports_range: true,
                etag: None,
                last_modified: None,
                content_type: None,
            })
        }

        async fn open_from(
            &self,
            _uri: &Url,
            offset: u64,
            _ctx: &OpenContext,
        ) -> Result<ByteStream> {
            let offset = offset as usize;
            let slice = if offset < self.data.len() {
                &self.data[offset..]
            } else {
                &[]
            };
            Ok(Box::pin(Cursor::new(slice.to_vec())))
        }

        fn name(&self) -> &'static str {
            "mock"
        }
    }

    /// A backend that tracks how many concurrent open_from calls are active.
    /// This is THE test that proves concurrency is real.
    #[derive(Debug)]
    struct ConcurrencyTrackingBackend {
        data: Vec<u8>,
        peak_concurrent: Arc<AtomicU32>,
        current_concurrent: Arc<AtomicU32>,
    }

    #[async_trait::async_trait]
    impl ByteSourceBackend for ConcurrencyTrackingBackend {
        async fn probe(&self, _uri: &Url, _ctx: &ProbeContext) -> Result<FileProbe> {
            Ok(FileProbe {
                size: Some(self.data.len() as u64),
                supports_range: true,
                etag: None,
                last_modified: None,
                content_type: None,
            })
        }

        async fn open_from(
            &self,
            _uri: &Url,
            offset: u64,
            _ctx: &OpenContext,
        ) -> Result<ByteStream> {
            // Increment current count, update peak.
            let prev = self.current_concurrent.fetch_add(1, Ordering::SeqCst);
            let current = prev + 1;
            self.peak_concurrent.fetch_max(current, Ordering::SeqCst);

            // Simulate network latency so multiple tasks overlap.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let offset = offset as usize;
            let slice = if offset < self.data.len() {
                self.data[offset..].to_vec()
            } else {
                vec![]
            };

            let current_concurrent = Arc::clone(&self.current_concurrent);

            // Wrap in a reader that decrements on drop.
            Ok(Box::pin(DecrementOnDropReader {
                inner: Cursor::new(slice),
                counter: current_concurrent,
            }))
        }

        fn name(&self) -> &'static str {
            "concurrency-tracking"
        }
    }

    /// An AsyncRead wrapper that decrements a counter when dropped.
    struct DecrementOnDropReader {
        inner: Cursor<Vec<u8>>,
        counter: Arc<AtomicU32>,
    }

    impl tokio::io::AsyncRead for DecrementOnDropReader {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let this = self.get_mut();
            std::pin::Pin::new(&mut this.inner).poll_read(cx, buf)
        }
    }

    impl Drop for DecrementOnDropReader {
        fn drop(&mut self) {
            self.counter.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// A backend that fails the first N calls, then succeeds. Tests retry.
    #[derive(Debug)]
    struct FlakeyBackend {
        data: Vec<u8>,
        fail_count: Arc<AtomicU32>,
        failures_remaining: Arc<AtomicU32>,
    }

    impl FlakeyBackend {
        fn new(data: Vec<u8>, failures: u32) -> Self {
            Self {
                data,
                fail_count: Arc::new(AtomicU32::new(0)),
                failures_remaining: Arc::new(AtomicU32::new(failures)),
            }
        }
    }

    #[async_trait::async_trait]
    impl ByteSourceBackend for FlakeyBackend {
        async fn probe(&self, _uri: &Url, _ctx: &ProbeContext) -> Result<FileProbe> {
            Ok(FileProbe {
                size: Some(self.data.len() as u64),
                supports_range: true,
                etag: None,
                last_modified: None,
                content_type: None,
            })
        }

        async fn open_from(
            &self,
            _uri: &Url,
            offset: u64,
            _ctx: &OpenContext,
        ) -> Result<ByteStream> {
            let remaining = self.failures_remaining.load(Ordering::SeqCst);
            if remaining > 0 {
                self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
                self.fail_count.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("simulated network error");
            }
            let offset = offset as usize;
            let slice = if offset < self.data.len() {
                &self.data[offset..]
            } else {
                &[]
            };
            Ok(Box::pin(Cursor::new(slice.to_vec())))
        }

        fn name(&self) -> &'static str {
            "flakey"
        }
    }

    fn noop_progress() -> Arc<dyn Fn(u32, u64) + Send + Sync> {
        Arc::new(|_, _| {})
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Basic single-segment download produces correct file
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn single_segment_correct_content() {
        let data = vec![42u8; 1000];
        let backend: Arc<dyn ByteSourceBackend> = Arc::new(MockBackend { data: data.clone() });

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(1000, 1);
        let mut segments = init_segment_states(&ranges);

        let executor = SegmentExecutor::new(ExecutorConfig::default());
        let cancel = CancellationToken::new();

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        apply_results(&mut segments, &results);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SegmentStatus::Done);
        assert_eq!(results[0].bytes_downloaded, 1000);
        assert_eq!(segments[0].status, SegmentStatus::Done);

        let written = std::fs::read(&out_path).unwrap();
        assert_eq!(written, data);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Multi-segment download assembles correct file
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn multi_segment_correct_content() {
        let data: Vec<u8> = (0..=255u8).cycle().take(10000).collect();
        let backend: Arc<dyn ByteSourceBackend> = Arc::new(MockBackend { data: data.clone() });

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(10000, 4);
        let mut segments = init_segment_states(&ranges);

        let executor = SegmentExecutor::new(ExecutorConfig {
            max_connections: 4,
            buffer_size: 1024,
            max_retries: 3,
            ..Default::default()
        });
        let cancel = CancellationToken::new();

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        apply_results(&mut segments, &results);

        assert_eq!(total_downloaded(&results), 10000);
        for seg in &segments {
            assert_eq!(seg.status, SegmentStatus::Done);
        }

        let written = std::fs::read(&out_path).unwrap();
        assert_eq!(written, data, "assembled file must match original byte-for-byte");
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Segments actually run concurrently (THE CRITICAL TEST)
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn segments_run_concurrently() {
        let data = vec![0u8; 4000]; // 4 segments of 1000 bytes each
        let peak = Arc::new(AtomicU32::new(0));
        let current = Arc::new(AtomicU32::new(0));

        let backend: Arc<dyn ByteSourceBackend> = Arc::new(ConcurrencyTrackingBackend {
            data,
            peak_concurrent: Arc::clone(&peak),
            current_concurrent: Arc::clone(&current),
        });

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(4000, 4);
        let segments = init_segment_states(&ranges);

        let executor = SegmentExecutor::new(ExecutorConfig {
            max_connections: 4,
            buffer_size: 512,
            max_retries: 0,
            ..Default::default()
        });
        let cancel = CancellationToken::new();

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        let peak_value = peak.load(Ordering::SeqCst);

        // With 4 segments, 4 max connections, and 50ms latency per open,
        // we MUST see peak > 1. If peak == 1, the executor is serial.
        assert!(
            peak_value > 1,
            "peak concurrent connections was {peak_value}, expected > 1. \
             The executor is NOT running concurrently!"
        );

        // All segments should complete successfully.
        assert_eq!(results.len(), 4);
        for r in &results {
            assert_eq!(
                r.status,
                SegmentStatus::Done,
                "segment {} failed: {:?}",
                r.segment_id,
                r.error
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Semaphore limits concurrency
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn semaphore_limits_concurrency() {
        let data = vec![0u8; 8000]; // 8 segments
        let peak = Arc::new(AtomicU32::new(0));
        let current = Arc::new(AtomicU32::new(0));

        let backend: Arc<dyn ByteSourceBackend> = Arc::new(ConcurrencyTrackingBackend {
            data,
            peak_concurrent: Arc::clone(&peak),
            current_concurrent: Arc::clone(&current),
        });

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(8000, 8);
        let segments = init_segment_states(&ranges);

        // Only allow 2 concurrent connections.
        let executor = SegmentExecutor::new(ExecutorConfig {
            max_connections: 2,
            buffer_size: 512,
            max_retries: 0,
            ..Default::default()
        });
        let cancel = CancellationToken::new();

        let _results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        let peak_value = peak.load(Ordering::SeqCst);

        // Peak should be at most 2 (the semaphore limit).
        assert!(
            peak_value <= 2,
            "peak concurrent was {peak_value}, but semaphore limit is 2"
        );
        // And it should actually reach 2 (proving parallelism + limiting).
        assert!(
            peak_value >= 2,
            "peak concurrent was {peak_value}, expected 2 — semaphore not saturating"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Retry on transient failure
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn retry_on_transient_failure() {
        let data = vec![7u8; 500];
        let flakey = FlakeyBackend::new(data.clone(), 2); // fail first 2 attempts
        let fail_count = Arc::clone(&flakey.fail_count);
        let backend: Arc<dyn ByteSourceBackend> = Arc::new(flakey);

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(500, 1);
        let segments = init_segment_states(&ranges);

        let executor = SegmentExecutor::new(ExecutorConfig {
            max_retries: 5,
            retry_base_delay_ms: 10, // fast for testing
            ..Default::default()
        });
        let cancel = CancellationToken::new();

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        // Should succeed after retries.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SegmentStatus::Done);
        assert_eq!(results[0].bytes_downloaded, 500);
        assert!(results[0].retries_used >= 2, "should have retried at least twice");

        // Verify the fail count was actually used.
        assert_eq!(fail_count.load(Ordering::SeqCst), 2);

        let written = std::fs::read(&out_path).unwrap();
        assert_eq!(written, data);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Permanent failure after max retries
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn permanent_failure_after_max_retries() {
        let data = vec![0u8; 500];
        let flakey = FlakeyBackend::new(data, 100); // always fails
        let backend: Arc<dyn ByteSourceBackend> = Arc::new(flakey);

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(500, 1);
        let segments = init_segment_states(&ranges);

        let executor = SegmentExecutor::new(ExecutorConfig {
            max_retries: 3,
            retry_base_delay_ms: 10,
            ..Default::default()
        });
        let cancel = CancellationToken::new();

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SegmentStatus::Failed);
        assert!(results[0].error.is_some());
        assert_eq!(results[0].retries_used, 3);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Cancellation stops all segments
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn cancellation_stops_all_segments() {
        let data = vec![0u8; 100_000];
        let backend: Arc<dyn ByteSourceBackend> = Arc::new(MockBackend { data });

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(100_000, 10);
        let segments = init_segment_states(&ranges);

        let executor = SegmentExecutor::new(ExecutorConfig::default());
        let cancel = CancellationToken::new();
        cancel.cancel(); // Cancel immediately.

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        let total = total_downloaded(&results);
        assert!(total < 100_000, "should not have downloaded everything after cancel");
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Progress callback receives all bytes
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn progress_callback_receives_all_bytes() {
        let data = vec![99u8; 2000];
        let backend: Arc<dyn ByteSourceBackend> = Arc::new(MockBackend { data });

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let ranges = plan_segments(2000, 2);
        let segments = init_segment_states(&ranges);

        let executor = SegmentExecutor::new(ExecutorConfig {
            buffer_size: 256,
            max_connections: 2,
            ..Default::default()
        });
        let cancel = CancellationToken::new();

        let progress_total = Arc::new(AtomicU64::new(0));
        let progress_count = Arc::new(AtomicU32::new(0));
        let pt = Arc::clone(&progress_total);
        let pc = Arc::clone(&progress_count);

        let on_progress: Arc<dyn Fn(u32, u64) + Send + Sync> =
            Arc::new(move |_seg_id, bytes| {
                pt.fetch_add(bytes, Ordering::Relaxed);
                pc.fetch_add(1, Ordering::Relaxed);
            });

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, on_progress)
            .await
            .unwrap();

        let reported = progress_total.load(Ordering::Relaxed);
        let count = progress_count.load(Ordering::Relaxed);

        assert_eq!(reported, 2000, "total progress bytes must equal file size");
        assert!(count > 1, "progress should be called multiple times");
        assert_eq!(total_downloaded(&results), 2000);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: Done segments are skipped
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn skips_done_segments() {
        let data = vec![0u8; 500];
        let backend: Arc<dyn ByteSourceBackend> = Arc::new(MockBackend { data });

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("output.bin");
        let uri: Url = "http://example.com/file".parse().unwrap();

        let mut segments = init_segment_states(&[(0, 250), (250, 500)]);
        segments[0].status = SegmentStatus::Done;
        segments[0].downloaded = 250;

        let executor = SegmentExecutor::new(ExecutorConfig::default());
        let cancel = CancellationToken::new();

        let results = executor
            .execute(backend, &uri, &out_path, &segments, cancel, noop_progress())
            .await
            .unwrap();

        // Only segment 1 should have been downloaded.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].segment_id, 1);
        assert_eq!(results[0].bytes_downloaded, 250);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: apply_results updates segments correctly
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn apply_results_updates_state() {
        let mut segments = init_segment_states(&[(0, 500), (500, 1000)]);

        let results = vec![
            SegmentResult {
                segment_id: 0,
                bytes_downloaded: 500,
                status: SegmentStatus::Done,
                error: None,
                retries_used: 0,
            },
            SegmentResult {
                segment_id: 1,
                bytes_downloaded: 300,
                status: SegmentStatus::Failed,
                error: Some("timeout".into()),
                retries_used: 3,
            },
        ];

        apply_results(&mut segments, &results);

        assert_eq!(segments[0].downloaded, 500);
        assert_eq!(segments[0].status, SegmentStatus::Done);
        assert_eq!(segments[1].downloaded, 300);
        assert_eq!(segments[1].status, SegmentStatus::Failed);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Test: total_downloaded sums correctly
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn total_downloaded_sums() {
        let results = vec![
            SegmentResult {
                segment_id: 0,
                bytes_downloaded: 100,
                status: SegmentStatus::Done,
                error: None,
                retries_used: 0,
            },
            SegmentResult {
                segment_id: 1,
                bytes_downloaded: 200,
                status: SegmentStatus::Done,
                error: None,
                retries_used: 0,
            },
        ];
        assert_eq!(total_downloaded(&results), 300);
    }
}
