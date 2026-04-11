// raria-core: Service layer — orchestration extracted from CLI.
//
// This module provides the download execution orchestration that was
// previously embedded in the CLI's daemon loop. It handles:
// - URI scheme detection and backend dispatch
// - Job activation and run loop
// - Rate limiter integration
//
// The service does NOT own network I/O directly — it delegates to
// protocol-specific backends (HttpBackend, FtpBackend, SftpBackend)
// via the ByteSourceBackend trait.

use crate::engine::Engine;
use crate::limiter::RateLimiter;
use std::sync::Arc;

/// Identifies the download source type from a URI string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSource {
    /// HTTP or HTTPS download.
    Http,
    /// FTP download.
    Ftp,
    /// FTPS (explicit TLS over FTP).
    Ftps,
    /// SFTP (SSH File Transfer Protocol).
    Sftp,
    /// BitTorrent magnet link.
    Magnet,
}

/// Heuristic classification for terminal download errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadErrorClass {
    /// The error may succeed on a later retry or different network path.
    Transient,
    /// The error is unlikely to succeed without changing the request/input.
    Permanent,
}

/// Detect the source type from a URI string.
///
/// Returns `None` if the URI scheme is unrecognized or the string is not a valid URL.
pub fn detect_scheme(uri: &str) -> Option<JobSource> {
    // Handle magnet links specially — they use "magnet:" not "magnet://"
    if uri.starts_with("magnet:") {
        return Some(JobSource::Magnet);
    }

    let url: url::Url = uri.parse().ok()?;
    match url.scheme() {
        "http" | "https" => Some(JobSource::Http),
        "ftp" => Some(JobSource::Ftp),
        "ftps" => Some(JobSource::Ftps),
        "sftp" => Some(JobSource::Sftp),
        _ => None,
    }
}

/// Heuristically classify a download error string as transient or permanent.
pub fn classify_download_error(message: &str) -> DownloadErrorClass {
    let normalized = message.to_ascii_lowercase();

    let permanent_markers = [
        "404",
        "not found",
        "checksum mismatch",
        "permission denied",
        "403",
        "401",
        "unsupported",
        "invalid uri",
        "invalid url",
        "bad request",
        "400",
        "failed to parse",
        "malformed",
    ];
    if permanent_markers.iter().any(|marker| normalized.contains(marker)) {
        return DownloadErrorClass::Permanent;
    }

    let transient_markers = [
        "timeout",
        "timed out",
        "connection reset",
        "connection refused",
        "connection aborted",
        "temporarily unavailable",
        "too many requests",
        "429",
        "502",
        "503",
        "504",
        "broken pipe",
    ];
    if transient_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return DownloadErrorClass::Transient;
    }

    DownloadErrorClass::Transient
}

/// The download service orchestrates job execution.
///
/// It wraps the Engine and manages the lifecycle of download tasks:
/// dispatching to the correct backend, managing rate limiting, and
/// reporting results back to the Engine.
pub struct DownloadService {
    engine: Arc<Engine>,
    rate_limiter: Option<Arc<RateLimiter>>,
}

impl DownloadService {
    /// Create a new DownloadService.
    pub fn new(engine: Arc<Engine>, rate_limiter: Option<Arc<RateLimiter>>) -> Self {
        Self {
            engine,
            rate_limiter,
        }
    }

    /// Get a reference to the underlying engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get a clone of the engine Arc.
    pub fn engine_arc(&self) -> Arc<Engine> {
        Arc::clone(&self.engine)
    }

    /// Get the rate limiter, if configured.
    pub fn rate_limiter(&self) -> Option<&Arc<RateLimiter>> {
        self.rate_limiter.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_scheme_http() {
        assert_eq!(
            detect_scheme("http://example.com/file"),
            Some(JobSource::Http)
        );
        assert_eq!(
            detect_scheme("https://example.com/file"),
            Some(JobSource::Http)
        );
    }

    #[test]
    fn detect_scheme_ftp() {
        assert_eq!(detect_scheme("ftp://host/path"), Some(JobSource::Ftp));
        assert_eq!(detect_scheme("ftps://host/path"), Some(JobSource::Ftps));
    }

    #[test]
    fn detect_scheme_sftp() {
        assert_eq!(detect_scheme("sftp://host/path"), Some(JobSource::Sftp));
    }

    #[test]
    fn detect_scheme_magnet() {
        assert_eq!(
            detect_scheme("magnet:?xt=urn:btih:abc123&dn=test"),
            Some(JobSource::Magnet)
        );
    }

    #[test]
    fn detect_scheme_unknown() {
        assert_eq!(detect_scheme("gopher://host/path"), None);
        assert_eq!(detect_scheme("not-a-url"), None);
        assert_eq!(detect_scheme(""), None);
    }

    #[test]
    fn classify_download_error_heuristics_cover_transient_and_permanent_cases() {
        assert_eq!(
            classify_download_error("http status 404 not found"),
            DownloadErrorClass::Permanent
        );
        assert_eq!(
            classify_download_error("checksum mismatch for file.bin"),
            DownloadErrorClass::Permanent
        );
        assert_eq!(
            classify_download_error("timeout while reading response body"),
            DownloadErrorClass::Transient
        );
        assert_eq!(
            classify_download_error("connection reset by peer"),
            DownloadErrorClass::Transient
        );
    }
}
