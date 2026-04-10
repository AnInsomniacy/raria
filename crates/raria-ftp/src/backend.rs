// raria-ftp: FTP/FTPS backend implementing ByteSourceBackend.
//
// Uses suppaftp for async FTP operations. Implements:
// - probe() → FTP SIZE command → FileProbe { size, supports_range: true }
// - open_from() → FTP resume_transfer(offset) + retr_as_stream → ByteStream
//
// Authentication is extracted from the URL:
//   ftp://user:pass@host:port/path
// If no credentials are given, "anonymous" / "" is used (standard FTP behavior).
//
// Design notes:
// - Each open_from() creates a new FTP connection. This is correct because FTP
//   is stateful and each data transfer requires its own control connection.
// - probe() creates a throwaway connection just for SIZE.
// - FTPS (TLS) is auto-negotiated via suppaftp's native-tls backend.

use anyhow::{Context, Result};
use async_trait::async_trait;
use raria_range::backend::{
    ByteSourceBackend, ByteStream, FileProbe, OpenContext, ProbeContext,
};
use std::pin::Pin;
use std::task::{self, Poll};
use suppaftp::tokio::AsyncNativeTlsFtpStream;
use suppaftp::types::FileType;
use tokio::io::AsyncRead;
use tracing::{debug, info, warn};
use url::Url;

/// Wraps an FTP control connection and its data stream together.
///
/// This ensures the FTP control connection stays alive as long as the data
/// stream is being read, and is properly cleaned up when dropped.
///
/// Previously, `mem::forget(ftp)` was used which leaked the control connection's
/// TCP socket and associated resources.
struct FtpOwnedStream<S: AsyncRead + Unpin> {
    /// The FTP control connection. Kept alive while data is being read.
    _ftp: AsyncNativeTlsFtpStream,
    /// The data stream from RETR.
    data: S,
}

impl<S: AsyncRead + Unpin> FtpOwnedStream<S> {
    fn new(ftp: AsyncNativeTlsFtpStream, data: S) -> Self {
        Self { _ftp: ftp, data }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for FtpOwnedStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // SAFETY: We only access `data` which is Unpin.
        let this = self.get_mut();
        Pin::new(&mut this.data).poll_read(cx, buf)
    }
}

// The wrapper is Send if both components are Send.
// AsyncNativeTlsFtpStream is Send, and the DataStream owns its socket.
unsafe impl<S: AsyncRead + Unpin + Send> Send for FtpOwnedStream<S> {}

/// FTP/FTPS download backend.
#[derive(Debug)]
pub struct FtpBackend;

impl FtpBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FtpBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract (host:port, user, password, path) from an FTP URL.
fn parse_ftp_url(uri: &Url) -> Result<(String, String, String, String)> {
    let host = uri.host_str().context("FTP URL missing host")?;
    let port = uri.port().unwrap_or(21);
    let addr = format!("{host}:{port}");

    let user = if uri.username().is_empty() {
        "anonymous".to_string()
    } else {
        uri.username().to_string()
    };
    let password = uri.password().unwrap_or("").to_string();
    let path = uri.path().to_string();

    Ok((addr, user, password, path))
}

/// Create an authenticated FTP connection.
async fn connect_ftp(uri: &Url) -> Result<(AsyncNativeTlsFtpStream, String)> {
    let (addr, user, password, path) = parse_ftp_url(uri)?;

    debug!(addr = %addr, user = %user, "connecting to FTP server");
    let mut ftp: AsyncNativeTlsFtpStream =
        AsyncNativeTlsFtpStream::connect(&addr)
            .await
            .with_context(|| format!("failed to connect to FTP server at {addr}"))?;

    ftp.login(&user, &password)
        .await
        .with_context(|| format!("FTP login failed for user '{user}'"))?;

    // Switch to binary (TYPE I) — required for accurate SIZE and byte transfers.
    ftp.transfer_type(FileType::Binary)
        .await
        .context("failed to set FTP transfer type to binary")?;

    debug!(path = %path, "FTP connection established");
    Ok((ftp, path))
}

#[async_trait]
impl ByteSourceBackend for FtpBackend {
    async fn probe(&self, uri: &Url, _ctx: &ProbeContext) -> Result<FileProbe> {
        debug!(uri = %uri, "probing FTP resource");

        let (mut ftp, path) = connect_ftp(uri).await?;

        // SIZE command to get file size. Not all servers support this.
        let size = match ftp.size(&path).await {
            Ok(size) => {
                info!(uri = %uri, size, "FTP SIZE succeeded");
                Some(size as u64)
            }
            Err(e) => {
                warn!(uri = %uri, error = %e, "FTP SIZE failed (server may not support it)");
                None
            }
        };

        // Clean up.
        let _ = ftp.quit().await;

        Ok(FileProbe {
            size,
            supports_range: true, // FTP always supports REST+RETR.
            etag: None,
            last_modified: None,
            content_type: None,
            suggested_filename: None,
            not_modified: false,
        })
    }

    async fn open_from(&self, uri: &Url, offset: u64, _ctx: &OpenContext) -> Result<ByteStream> {
        debug!(uri = %uri, offset, "opening FTP stream");

        let (mut ftp, path) = connect_ftp(uri).await?;

        // If offset > 0, send REST command to resume from that point.
        if offset > 0 {
            ftp.resume_transfer(offset as usize)
                .await
                .with_context(|| format!("FTP REST({offset}) failed"))?;
            debug!(offset, "FTP REST set successfully");
        }

        // RETR returns a DataStream that owns its data-channel socket.
        // The FTP control connection is needed for finalize_retr_stream()
        // after the data is read. We wrap both in FtpOwnedStream so the
        // control connection is properly cleaned up when the stream drops.
        let data_stream = ftp
            .retr_as_stream(&path)
            .await
            .with_context(|| format!("FTP RETR failed for {path}"))?;

        Ok(Box::pin(FtpOwnedStream::new(ftp, data_stream)))
    }

    fn name(&self) -> &'static str {
        "ftp"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ftp_backend_creates_successfully() {
        let backend = FtpBackend::new();
        assert_eq!(backend.name(), "ftp");
    }

    #[test]
    fn parse_ftp_url_with_credentials() {
        let url: Url = "ftp://user:secret@ftp.example.com:2121/pub/file.zip"
            .parse()
            .unwrap();
        let (addr, user, pass, path) = parse_ftp_url(&url).unwrap();
        assert_eq!(addr, "ftp.example.com:2121");
        assert_eq!(user, "user");
        assert_eq!(pass, "secret");
        assert_eq!(path, "/pub/file.zip");
    }

    #[test]
    fn parse_ftp_url_anonymous() {
        let url: Url = "ftp://ftp.example.com/pub/file.zip".parse().unwrap();
        let (addr, user, pass, path) = parse_ftp_url(&url).unwrap();
        assert_eq!(addr, "ftp.example.com:21");
        assert_eq!(user, "anonymous");
        assert_eq!(pass, "");
        assert_eq!(path, "/pub/file.zip");
    }

    #[test]
    fn parse_ftp_url_default_port() {
        let url: Url = "ftp://ftp.example.com/data/test.bin".parse().unwrap();
        let (addr, _, _, _) = parse_ftp_url(&url).unwrap();
        assert!(addr.ends_with(":21"));
    }

    #[test]
    fn parse_ftp_url_custom_port() {
        let url: Url = "ftp://ftp.example.com:990/data/test.bin".parse().unwrap();
        let (addr, _, _, _) = parse_ftp_url(&url).unwrap();
        assert_eq!(addr, "ftp.example.com:990");
    }

    #[test]
    fn parse_ftp_url_encoded_password() {
        let url: Url = "ftp://user:p%40ssword@ftp.example.com/f.zip"
            .parse()
            .unwrap();
        let (_, _, pass, _) = parse_ftp_url(&url).unwrap();
        assert_eq!(pass, "p%40ssword");
    }

    #[test]
    fn parse_ftp_url_root_path() {
        let url: Url = "ftp://ftp.example.com/".parse().unwrap();
        let (_, _, _, path) = parse_ftp_url(&url).unwrap();
        assert_eq!(path, "/");
    }

    #[test]
    fn parse_ftp_url_deep_path() {
        let url: Url = "ftp://ftp.example.com/a/b/c/d/file.tar.gz"
            .parse()
            .unwrap();
        let (_, _, _, path) = parse_ftp_url(&url).unwrap();
        assert_eq!(path, "/a/b/c/d/file.tar.gz");
    }
}
