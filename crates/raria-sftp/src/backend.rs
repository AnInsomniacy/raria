// raria-sftp: SFTP download backend implementing ByteSourceBackend.
//
// Uses russh + russh-sftp for SSH-based file transfer. Supports:
// - stat for file size probing
// - read_from(offset) for offset-based downloads

use anyhow::Result;
use async_trait::async_trait;
use raria_range::backend::{
    ByteSourceBackend, ByteStream, FileProbe, OpenContext, ProbeContext,
};
use tracing::debug;
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

#[async_trait]
impl ByteSourceBackend for SftpBackend {
    async fn probe(&self, uri: &Url, _ctx: &ProbeContext) -> Result<FileProbe> {
        debug!(uri = %uri, "probing SFTP resource");

        // TODO: Implement SFTP stat command.
        Ok(FileProbe {
            size: None,
            supports_range: true,
            etag: None,
            last_modified: None,
            content_type: None,
        })
    }

    async fn open_from(&self, uri: &Url, offset: u64, _ctx: &OpenContext) -> Result<ByteStream> {
        debug!(uri = %uri, offset, "opening SFTP stream");

        // TODO: Implement russh + russh-sftp read_from(offset).
        anyhow::bail!("SFTP backend not yet implemented")
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

    #[tokio::test]
    async fn sftp_probe_returns_range_support() {
        let backend = SftpBackend::new();
        let uri: Url = "sftp://server.example.com/file.zip".parse().unwrap();
        let probe = backend.probe(&uri, &ProbeContext::default()).await.unwrap();
        assert!(probe.supports_range); // SFTP always supports offset reads.
    }
}
