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
use russh::keys::{check_known_hosts_path, load_secret_key, PrivateKeyWithHashAlg};
use russh_sftp::client::SftpSession;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncSeekExt;
use tracing::{debug, info};
use url::Url;

/// SFTP-specific backend configuration.
#[derive(Debug, Clone, Default)]
pub struct SftpBackendConfig {
    /// When true, reject servers whose host key is not present in the known_hosts file.
    pub strict_host_key_check: bool,
    /// Optional non-default known_hosts file path.
    pub known_hosts_path: Option<PathBuf>,
    /// Optional private key path for public-key authentication.
    pub private_key_path: Option<PathBuf>,
    /// Optional passphrase for the private key.
    pub private_key_passphrase: Option<String>,
}

/// SFTP download backend.
#[derive(Debug, Clone)]
pub struct SftpBackend {
    config: SftpBackendConfig,
}

impl SftpBackend {
    pub fn new() -> Self {
        Self::with_config(SftpBackendConfig::default())
    }

    pub fn with_config(config: SftpBackendConfig) -> Self {
        Self { config }
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

/// Minimal SSH client handler with optional known_hosts verification.
struct SshHandler {
    host: String,
    port: u16,
    config: SftpBackendConfig,
}

impl SshHandler {
    fn new(host: String, port: u16, config: SftpBackendConfig) -> Self {
        Self { host, port, config }
    }
}

fn verify_known_host(
    host: &str,
    port: u16,
    server_public_key: &russh::keys::PublicKey,
    config: &SftpBackendConfig,
) -> Result<bool> {
    if !config.strict_host_key_check {
        return Ok(true);
    }

    if let Some(ref path) = config.known_hosts_path {
        Ok(check_known_hosts_path(host, port, server_public_key, path)?)
    } else {
        Ok(russh::keys::check_known_hosts(host, port, server_public_key)?)
    }
}

impl client::Handler for SshHandler {
    type Error = anyhow::Error;

    #[allow(clippy::manual_async_fn)]
    fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let host = self.host.clone();
        let port = self.port;
        let config = self.config.clone();
        async move { verify_known_host(&host, port, server_public_key, &config) }
    }
}

/// Establish an SFTP session over SSH.
async fn connect_sftp(uri: &Url, backend_config: &SftpBackendConfig) -> Result<(SftpSession, String)> {
    let (host, port, user, password, path) = parse_sftp_url(uri)?;

    debug!(host = %host, port, user = %user, "connecting via SSH");

    let ssh_config = Arc::new(client::Config::default());
    let handler = SshHandler::new(host.clone(), port, backend_config.clone());

    let mut session = client::connect(ssh_config, (host.as_str(), port), handler)
        .await
        .with_context(|| format!("SSH connection failed to {host}:{port}"))?;

    // Authenticate with private key when configured, otherwise fall back to password.
    let auth_result = if let Some(ref private_key_path) = backend_config.private_key_path {
        let key = load_secret_key(
            private_key_path,
            backend_config.private_key_passphrase.as_deref(),
        )
        .with_context(|| format!("failed to load SSH private key: {}", private_key_path.display()))?;
        session
            .authenticate_publickey(&user, PrivateKeyWithHashAlg::new(Arc::new(key), None))
            .await
            .with_context(|| format!("SSH public-key auth failed for user '{user}'"))?
    } else {
        session
            .authenticate_password(&user, &password)
            .await
            .with_context(|| format!("SSH auth failed for user '{user}'"))?
    };

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

        let (sftp, path) = connect_sftp(uri, &self.config).await?;

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
            not_modified: false,
        })
    }

    async fn open_from(&self, uri: &Url, offset: u64, _ctx: &OpenContext) -> Result<ByteStream> {
        debug!(uri = %uri, offset, "opening SFTP stream");

        let (sftp, path) = connect_sftp(uri, &self.config).await?;

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
    use russh::keys::parse_public_key_base64;

    #[test]
    fn sftp_backend_creates_successfully() {
        let backend = SftpBackend::new();
        assert_eq!(backend.name(), "sftp");
    }

    #[test]
    fn sftp_backend_with_config_preserves_settings() {
        let backend = SftpBackend::with_config(SftpBackendConfig {
            strict_host_key_check: true,
            known_hosts_path: Some(PathBuf::from("/tmp/known_hosts")),
            private_key_path: Some(PathBuf::from("/tmp/id_ed25519")),
            private_key_passphrase: Some("secret".into()),
        });
        assert!(backend.config.strict_host_key_check);
        assert_eq!(backend.config.known_hosts_path, Some(PathBuf::from("/tmp/known_hosts")));
        assert_eq!(backend.config.private_key_path, Some(PathBuf::from("/tmp/id_ed25519")));
        assert_eq!(backend.config.private_key_passphrase.as_deref(), Some("secret"));
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

    #[test]
    fn verify_known_host_accepts_all_when_strict_check_is_disabled() {
        let key = parse_public_key_base64(
            "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
        )
        .unwrap();
        let allowed = verify_known_host(
            "localhost",
            22,
            &key,
            &SftpBackendConfig {
                strict_host_key_check: false,
                known_hosts_path: None,
                private_key_path: None,
                private_key_passphrase: None,
            },
        )
        .unwrap();
        assert!(allowed);
    }

    #[test]
    fn verify_known_host_checks_custom_known_hosts_file_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        std::fs::write(
            &path,
            "localhost ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ\n",
        )
        .unwrap();
        let key = parse_public_key_base64(
            "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
        )
        .unwrap();
        let allowed = verify_known_host(
            "localhost",
            22,
            &key,
            &SftpBackendConfig {
                strict_host_key_check: true,
                known_hosts_path: Some(path),
                private_key_path: None,
                private_key_passphrase: None,
            },
        )
        .unwrap();
        assert!(allowed);
    }

    #[test]
    fn verify_known_host_rejects_missing_entry_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        std::fs::write(
            &path,
            "otherhost ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ\n",
        )
        .unwrap();
        let key = parse_public_key_base64(
            "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
        )
        .unwrap();
        let allowed = verify_known_host(
            "localhost",
            22,
            &key,
            &SftpBackendConfig {
                strict_host_key_check: true,
                known_hosts_path: Some(path),
                private_key_path: None,
                private_key_passphrase: None,
            },
        )
        .unwrap();
        assert!(!allowed);
    }
}
