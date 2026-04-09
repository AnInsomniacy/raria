// raria-sftp: SFTP download backend implementing ByteSourceBackend.
//
// Uses russh for SSH transport and russh-sftp for SFTP operations.
//
// Implements:
// - probe() → SFTP stat → FileProbe { size, supports_range: true }
// - open_from() → SFTP open + seek(offset) → ByteStream (AsyncRead)
//
// Authentication is extracted from the URL:
//   sftp://user:pass@host:port/path
//
// Design notes:
// - Each probe() and open_from() creates a fresh SSH connection.
//   SFTP is stateful per-session. This matches aria2's behavior where each
//   segment task independently connects to the server.
// - For key-based auth, the URL should still contain the username.
//   Password auth is the default; key auth support is a future enhancement.

use anyhow::{Context, Result};
use async_trait::async_trait;
use raria_range::backend::{
    ByteSourceBackend, ByteStream, FileProbe, OpenContext, ProbeContext,
};
use russh::client;
use russh_sftp::client::SftpSession;
use std::sync::Arc;
use tokio::io::AsyncSeekExt;
use tracing::{debug, info};
use url::Url;

/// SFTP download backend.
#[derive(Debug)]
pub struct SftpBackend;

impl SftpBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SftpBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract (host, port, user, password, path) from an SFTP URL.
fn parse_sftp_url(uri: &Url) -> Result<(String, u16, String, String, String)> {
    let host = uri
        .host_str()
        .context("SFTP URL missing host")?
        .to_string();
    let port = uri.port().unwrap_or(22);

    let user = if uri.username().is_empty() {
        "root".to_string()
    } else {
        uri.username().to_string()
    };
    let password = uri.password().unwrap_or("").to_string();
    let path = uri.path().to_string();

    Ok((host, port, user, password, path))
}

/// Minimal SSH client handler.
/// Accepts all host keys (like aria2's default behavior).
/// Production deployments should add known_hosts verification.
struct SshHandler;

impl client::Handler for SshHandler {
    type Error = anyhow::Error;

    #[allow(clippy::manual_async_fn)]
    fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        // Accept all host keys — matches aria2's default.
        async { Ok(true) }
    }
}

/// Establish an SFTP session over SSH.
async fn connect_sftp(uri: &Url) -> Result<(SftpSession, String)> {
    let (host, port, user, password, path) = parse_sftp_url(uri)?;

    debug!(host = %host, port, user = %user, "connecting via SSH");

    let config = Arc::new(client::Config::default());
    let handler = SshHandler;

    let mut session = client::connect(config, (host.as_str(), port), handler)
        .await
        .with_context(|| format!("SSH connection failed to {host}:{port}"))?;

    // Authenticate with password.
    let auth_result = session
        .authenticate_password(&user, &password)
        .await
        .with_context(|| format!("SSH auth failed for user '{user}'"))?;

    if !auth_result.success() {
        anyhow::bail!("SSH password authentication rejected for user '{user}'");
    }

    // Open SFTP channel.
    let channel = session
        .channel_open_session()
        .await
        .context("failed to open SSH channel")?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .context("failed to request SFTP subsystem")?;

    let sftp = SftpSession::new(channel.into_stream())
        .await
        .context("failed to initialize SFTP session")?;

    debug!(path = %path, "SFTP session established");
    Ok((sftp, path))
}

#[async_trait]
impl ByteSourceBackend for SftpBackend {
    async fn probe(&self, uri: &Url, _ctx: &ProbeContext) -> Result<FileProbe> {
        debug!(uri = %uri, "probing SFTP resource");

        let (sftp, path) = connect_sftp(uri).await?;

        // stat() to get file metadata.
        let metadata = sftp
            .metadata(&path)
            .await
            .with_context(|| format!("SFTP stat failed for {path}"))?;

        let size = metadata.size;
        info!(uri = %uri, size = ?size, "SFTP stat succeeded");

        Ok(FileProbe {
            size,
            supports_range: true, // SFTP always supports seek + read.
            etag: None,
            last_modified: None,
            content_type: None,
            suggested_filename: None,
        })
    }

    async fn open_from(&self, uri: &Url, offset: u64, _ctx: &OpenContext) -> Result<ByteStream> {
        debug!(uri = %uri, offset, "opening SFTP stream");

        let (sftp, path) = connect_sftp(uri).await?;

        // Open the remote file for reading.
        let mut file = sftp
            .open(&path)
            .await
            .with_context(|| format!("SFTP open failed for {path}"))?;

        // Seek to the requested offset.
        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .with_context(|| format!("SFTP seek to {offset} failed"))?;
            debug!(offset, "SFTP seek succeeded");
        }

        // The SFTP file handle implements AsyncRead + Send, which matches ByteStream.
        Ok(Box::pin(file))
    }

    fn name(&self) -> &'static str {
        "sftp"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sftp_backend_creates_successfully() {
        let backend = SftpBackend::new();
        assert_eq!(backend.name(), "sftp");
    }

    #[test]
    fn parse_sftp_url_with_credentials() {
        let url: Url = "sftp://user:secret@server.example.com:2222/home/user/file.zip"
            .parse()
            .unwrap();
        let (host, port, user, pass, path) = parse_sftp_url(&url).unwrap();
        assert_eq!(host, "server.example.com");
        assert_eq!(port, 2222);
        assert_eq!(user, "user");
        assert_eq!(pass, "secret");
        assert_eq!(path, "/home/user/file.zip");
    }

    #[test]
    fn parse_sftp_url_default_port() {
        let url: Url = "sftp://user@server.example.com/file.zip".parse().unwrap();
        let (_, port, _, _, _) = parse_sftp_url(&url).unwrap();
        assert_eq!(port, 22);
    }

    #[test]
    fn parse_sftp_url_default_user() {
        let url: Url = "sftp://server.example.com/file.zip".parse().unwrap();
        let (_, _, user, _, _) = parse_sftp_url(&url).unwrap();
        assert_eq!(user, "root");
    }

    #[test]
    fn parse_sftp_url_deep_path() {
        let url: Url = "sftp://user@host/a/b/c/file.tar.gz".parse().unwrap();
        let (_, _, _, _, path) = parse_sftp_url(&url).unwrap();
        assert_eq!(path, "/a/b/c/file.tar.gz");
    }

    #[test]
    fn parse_sftp_url_no_password() {
        let url: Url = "sftp://user@host/file".parse().unwrap();
        let (_, _, _, pass, _) = parse_sftp_url(&url).unwrap();
        assert_eq!(pass, "");
    }
}
