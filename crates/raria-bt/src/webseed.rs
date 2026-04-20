//! WebSeed pre-download service.
//!
//! Downloads torrent files via HTTP/FTP/SFTP **before** librqbit starts,
//! placing them into the output directory so that librqbit's `initial_check`
//! discovers them as already-complete pieces.
//!
//! This module orchestrates file-level downloads using
//! [`raria_range::executor::SegmentExecutor`] and verifies each piece with
//! SHA-1 hashing — no modification to upstream librqbit required.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use raria_core::segment::{init_segment_states, plan_segments};
use raria_range::backend::ByteSourceBackend;
use raria_range::executor::{ExecutorConfig, SegmentExecutor, apply_results};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::torrent_meta::TorrentMeta;

// ----- Public types -----

/// Configuration for a WebSeed pre-download operation.
pub struct WebSeedConfig {
    /// Maximum number of concurrent download connections per file.
    pub max_connections: u32,
    /// Timeout for individual stream operations.
    pub timeout: Duration,
    /// Cancellation token to abort the pre-download.
    pub cancel: CancellationToken,
}

/// Result of a WebSeed pre-download operation.
#[derive(Debug, Default)]
pub struct WebSeedResult {
    /// Number of pieces that passed SHA-1 verification.
    pub pieces_verified: u32,
    /// Number of pieces that failed SHA-1 verification.
    pub pieces_failed: u32,
    /// Total bytes downloaded across all files.
    pub bytes_downloaded: u64,
}

// ----- Public API -----

/// Pre-download torrent files via WebSeed URIs.
///
/// For each file in the torrent, selects a WebSeed backend (HTTP/FTP/SFTP),
/// downloads via `SegmentExecutor`, and verifies piece hashes.
///
/// Files are written to `output_dir` with the correct directory structure
/// so that librqbit's `initial_check` can discover them.
pub async fn pre_download(
    meta: &TorrentMeta,
    output_dir: &Path,
    config: &WebSeedConfig,
) -> Result<WebSeedResult> {
    if meta.web_seed_uris.is_empty() {
        return Ok(WebSeedResult::default());
    }

    info!(
        num_files = meta.files.len(),
        num_uris = meta.web_seed_uris.len(),
        total_bytes = meta.total_length(),
        "starting WebSeed pre-download"
    );

    let mut result = WebSeedResult::default();

    for (file_idx, file) in meta.files.iter().enumerate() {
        if config.cancel.is_cancelled() {
            warn!("WebSeed pre-download cancelled");
            break;
        }

        // Build candidate download URLs for this file.
        let urls = candidate_urls_for_file(meta, file_idx);
        if urls.is_empty() {
            warn!(
                file = %file.path.display(),
                "no valid WebSeed URLs for file, skipping"
            );
            continue;
        }

        // Ensure parent directories exist.
        let file_path = output_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create dir for {}", file.path.display()))?;
        }

        // Try each URL until one succeeds.
        let mut downloaded = false;
        for url in &urls {
            let backend = match create_backend_for_url(url) {
                Ok(b) => b,
                Err(e) => {
                    warn!(%url, error = %e, "failed to create backend, trying next URL");
                    continue;
                }
            };

            let exec_config = ExecutorConfig {
                max_connections: config.max_connections,
                buffer_size: 64 * 1024,
                max_retries: 3,
                retry_base_delay_ms: 1000,
                rate_limiter: None,
                on_checkpoint: None,
                file_allocation: raria_core::file_alloc::FileAllocation::None,
                request_timeout: config.timeout,
                request_headers: Vec::new(),
                request_auth: None,
                request_etag: None,
                lowest_speed_limit_bps: 0,
                lowest_speed_grace: Duration::from_secs(10),
                max_file_not_found: 3,
            };

            // Plan segments for this file.
            let num_segments = config.max_connections.max(1);
            let ranges = plan_segments(file.length, num_segments);
            let mut segments = init_segment_states(&ranges);

            let executor = SegmentExecutor::new(exec_config);
            let noop_progress: Arc<dyn Fn(u32, u64) + Send + Sync> = Arc::new(|_seg_id, _bytes| {});

            debug!(%url, file = %file.path.display(), "downloading via WebSeed");
            match executor
                .execute(
                    backend,
                    &url.clone(),
                    &file_path,
                    &segments,
                    config.cancel.clone(),
                    noop_progress,
                )
                .await
            {
                Ok(seg_results) => {
                    apply_results(&mut segments, &seg_results);
                    let file_bytes: u64 = seg_results.iter().map(|r| r.bytes_downloaded).sum();
                    result.bytes_downloaded += file_bytes;
                    downloaded = true;
                    debug!(
                        %url,
                        file = %file.path.display(),
                        bytes = file_bytes,
                        "WebSeed file download complete"
                    );
                    break;
                }
                Err(e) => {
                    warn!(%url, error = %e, "WebSeed download failed, trying next URL");
                    // Clean up partial file.
                    let _ = tokio::fs::remove_file(&file_path).await;
                    continue;
                }
            }
        }

        if !downloaded {
            warn!(
                file = %file.path.display(),
                "all WebSeed URLs failed for file"
            );
        }
    }

    // Verify pieces.
    verify_pieces(meta, output_dir, &mut result).await?;

    info!(
        verified = result.pieces_verified,
        failed = result.pieces_failed,
        bytes = result.bytes_downloaded,
        "WebSeed pre-download finished"
    );

    Ok(result)
}

// ----- URL construction -----

/// Build candidate download URLs for a specific file in the torrent.
///
/// For single-file torrents (BEP-19 style), the WebSeed URL is used as-is
/// if it already contains the filename, otherwise the filename is appended.
///
/// For multi-file torrents, the torrent name and file path are appended
/// to the base URL.
fn candidate_urls_for_file(meta: &TorrentMeta, file_index: usize) -> Vec<url::Url> {
    let file = &meta.files[file_index];
    let mut urls = Vec::new();

    for base_url in &meta.web_seed_uris {
        let joined = if meta.is_single_file {
            // BEP-19: For single-file, the URL might point directly to the file.
            let base_str = base_url.as_str();
            if base_str.ends_with('/') {
                // Base URL is a directory — append filename.
                match url::Url::parse(&format!("{}{}", base_str, file.path.display())) {
                    Ok(u) => u,
                    Err(_) => continue,
                }
            } else {
                // Base URL points to the file directly.
                base_url.clone()
            }
        } else {
            // Multi-file: base_url / torrent_name / relative_path
            let base_str = base_url.as_str();
            let separator = if base_str.ends_with('/') { "" } else { "/" };
            let rel_path = file.path.display();
            let full = format!("{base_str}{separator}{name}/{rel_path}", name = meta.name);
            match url::Url::parse(&full) {
                Ok(u) => u,
                Err(_) => continue,
            }
        };
        urls.push(joined);
    }

    urls
}

/// Create a `ByteSourceBackend` for the given URL based on its scheme.
fn create_backend_for_url(url: &url::Url) -> Result<Arc<dyn ByteSourceBackend>> {
    match url.scheme() {
        "http" | "https" => {
            let backend = raria_http::backend::HttpBackend::new().context("create HTTP backend")?;
            Ok(Arc::new(backend))
        }
        "ftp" | "ftps" => {
            let backend = raria_ftp::backend::FtpBackend::new();
            Ok(Arc::new(backend))
        }
        "sftp" => {
            let backend = raria_sftp::backend::SftpBackend::new();
            Ok(Arc::new(backend))
        }
        other => {
            anyhow::bail!("unsupported WebSeed URI scheme: {other}");
        }
    }
}

// ----- Piece verification -----

/// Read pieces from downloaded files and verify SHA-1 hashes.
async fn verify_pieces(
    meta: &TorrentMeta,
    output_dir: &Path,
    result: &mut WebSeedResult,
) -> Result<()> {
    for piece_idx in 0..meta.num_pieces() {
        let piece_size = meta.piece_size(piece_idx) as usize;
        let ranges = meta.piece_file_ranges(piece_idx);

        let mut piece_data = Vec::with_capacity(piece_size);
        let mut all_readable = true;

        for range in &ranges {
            let file = &meta.files[range.file_index];
            let file_path = output_dir.join(&file.path);
            match read_file_range(&file_path, range.file_offset, range.length).await {
                Ok(data) => piece_data.extend_from_slice(&data),
                Err(_) => {
                    all_readable = false;
                    break;
                }
            }
        }

        if !all_readable || piece_data.len() != piece_size {
            result.pieces_failed += 1;
            continue;
        }

        if meta.verify_piece(piece_idx, &piece_data) {
            result.pieces_verified += 1;
        } else {
            result.pieces_failed += 1;
            // librqbit's initial_check will handle partial verification
            // and re-download corrupted pieces via BT peers.
            debug!(
                piece = piece_idx,
                "piece SHA-1 mismatch, will fallback to BT"
            );
        }
    }

    Ok(())
}

/// Read a byte range from a file.
async fn read_file_range(path: &Path, offset: u64, length: u64) -> Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut buf = vec![0u8; length as usize];
    file.read_exact(&mut buf).await?;
    Ok(buf)
}

// ----- Unit tests -----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_urls_single_file_direct() {
        let meta = crate::torrent_meta::TorrentMeta {
            name: "test.bin".to_string(),
            files: vec![crate::torrent_meta::TorrentFile {
                path: "test.bin".into(),
                length: 1024,
                offset: 0,
            }],
            piece_length: 512,
            piece_hashes: vec![[0u8; 20]; 2],
            web_seed_uris: vec![url::Url::parse("https://example.com/test.bin").unwrap()],
            is_single_file: true,
        };

        let urls = candidate_urls_for_file(&meta, 0);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].as_str(), "https://example.com/test.bin");
    }

    #[test]
    fn candidate_urls_single_file_directory_base() {
        let meta = crate::torrent_meta::TorrentMeta {
            name: "test.bin".to_string(),
            files: vec![crate::torrent_meta::TorrentFile {
                path: "test.bin".into(),
                length: 1024,
                offset: 0,
            }],
            piece_length: 512,
            piece_hashes: vec![[0u8; 20]; 2],
            web_seed_uris: vec![url::Url::parse("https://example.com/files/").unwrap()],
            is_single_file: true,
        };

        let urls = candidate_urls_for_file(&meta, 0);
        assert_eq!(urls[0].as_str(), "https://example.com/files/test.bin");
    }

    #[test]
    fn candidate_urls_multi_file() {
        let meta = crate::torrent_meta::TorrentMeta {
            name: "my-torrent".to_string(),
            files: vec![crate::torrent_meta::TorrentFile {
                path: "subdir/file.txt".into(),
                length: 100,
                offset: 0,
            }],
            piece_length: 256,
            piece_hashes: vec![[0u8; 20]],
            web_seed_uris: vec![url::Url::parse("https://mirror.example.com/").unwrap()],
            is_single_file: false,
        };

        let urls = candidate_urls_for_file(&meta, 0);
        assert_eq!(
            urls[0].as_str(),
            "https://mirror.example.com/my-torrent/subdir/file.txt"
        );
    }
}
