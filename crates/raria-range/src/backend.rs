// raria-range: ByteSourceBackend trait definition.
//
// This trait abstracts over HTTP, FTP, and SFTP protocols, providing a
// uniform interface for probing file metadata and opening byte streams
// from arbitrary offsets.

use anyhow::Result;
use async_trait::async_trait;
use std::fmt;
use std::pin::Pin;
use std::time::Duration;
use tokio::io::AsyncRead;
use url::Url;

/// Context for probing file metadata.
#[derive(Debug, Clone)]
pub struct ProbeContext {
    /// Custom HTTP headers (ignored by FTP/SFTP backends).
    pub headers: Vec<(String, String)>,
    /// Authentication credentials.
    pub auth: Option<Credentials>,
    /// Probe request timeout.
    pub timeout: Duration,
}

impl Default for ProbeContext {
    fn default() -> Self {
        Self {
            headers: Vec::new(),
            auth: None,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Context for opening a download stream.
#[derive(Debug, Clone)]
pub struct OpenContext {
    /// Authentication credentials.
    pub auth: Option<Credentials>,
    /// Read timeout per data chunk.
    pub timeout: Duration,
}

impl Default for OpenContext {
    fn default() -> Self {
        Self {
            auth: None,
            timeout: Duration::from_secs(60),
        }
    }
}

/// Authentication credentials.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Metadata about a remote file obtained via probing.
#[derive(Debug, Clone)]
pub struct FileProbe {
    /// File size in bytes, if the server reported it.
    pub size: Option<u64>,
    /// Whether the server supports byte-range requests (HTTP Range / FTP REST).
    pub supports_range: bool,
    /// ETag for conditional requests (HTTP only).
    pub etag: Option<String>,
    /// Last-Modified header value (HTTP only).
    pub last_modified: Option<String>,
    /// Content-Type (HTTP only).
    pub content_type: Option<String>,
    /// Suggested filename from Content-Disposition header (HTTP only).
    pub suggested_filename: Option<String>,
}

/// A boxed async byte stream.
pub type ByteStream = Pin<Box<dyn AsyncRead + Send>>;

/// Abstraction for protocols that support byte-offset downloads.
///
/// Implementations exist for HTTP (`raria-http`), FTP (`raria-ftp`),
/// and SFTP (`raria-sftp`).
///
/// The key insight: `open_from(offset)` returns a forward-only stream
/// with no upper bound. The caller (SegmentExecutor) is responsible for
/// consuming only the bytes it needs and then dropping the stream.
///
/// This matches the natural semantics of:
/// - HTTP: `Range: bytes=offset-`
/// - FTP: `REST offset` + `RETR`
/// - SFTP: `read_from(offset)`
#[async_trait]
pub trait ByteSourceBackend: Send + Sync + fmt::Debug {
    /// Probe file metadata without downloading content.
    async fn probe(&self, uri: &Url, ctx: &ProbeContext) -> Result<FileProbe>;

    /// Open a byte stream starting from the given offset.
    ///
    /// The stream reads forward from `offset` until EOF or the caller
    /// stops consuming. There is no upper-bound parameter because the
    /// SegmentExecutor controls how many bytes to read.
    async fn open_from(&self, uri: &Url, offset: u64, ctx: &OpenContext) -> Result<ByteStream>;

    /// Human-readable name for this backend (e.g., "http", "ftp", "sftp").
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_context_default() {
        let ctx = ProbeContext::default();
        assert!(ctx.headers.is_empty());
        assert!(ctx.auth.is_none());
        assert_eq!(ctx.timeout, Duration::from_secs(30));
    }

    #[test]
    fn open_context_default() {
        let ctx = OpenContext::default();
        assert!(ctx.auth.is_none());
        assert_eq!(ctx.timeout, Duration::from_secs(60));
    }

    #[test]
    fn file_probe_construction() {
        let probe = FileProbe {
            size: Some(1024),
            supports_range: true,
            etag: Some("abc".into()),
            last_modified: None,
            content_type: Some("application/octet-stream".into()),
            suggested_filename: None,
        };
        assert_eq!(probe.size, Some(1024));
        assert!(probe.supports_range);
    }
}
