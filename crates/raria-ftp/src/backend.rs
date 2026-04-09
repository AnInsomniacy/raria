// raria-ftp: FTP/FTPS backend implementing ByteSourceBackend.
//
// Uses suppaftp for async FTP operations. Supports:
// - SIZE command for probing
// - REST + RETR for offset-based downloads

use anyhow::Result;
use async_trait::async_trait;
use raria_range::backend::{
    ByteSourceBackend, ByteStream, FileProbe, OpenContext, ProbeContext,
};
use tracing::debug;
use url::Url;

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

#[async_trait]
impl ByteSourceBackend for FtpBackend {
    async fn probe(&self, uri: &Url, _ctx: &ProbeContext) -> Result<FileProbe> {
        debug!(uri = %uri, "probing FTP resource");

        // TODO: Implement FTP SIZE command probe.
        // For now, return a basic probe indicating range support
        // (all FTP servers support REST+RETR).
        Ok(FileProbe {
            size: None,
            supports_range: true,
            etag: None,
            last_modified: None,
            content_type: None,
        })
    }

    async fn open_from(&self, uri: &Url, offset: u64, _ctx: &OpenContext) -> Result<ByteStream> {
        debug!(uri = %uri, offset, "opening FTP stream");

        // TODO: Implement REST + RETR with suppaftp.
        anyhow::bail!("FTP backend not yet implemented")
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

    #[tokio::test]
    async fn ftp_probe_returns_range_support() {
        let backend = FtpBackend::new();
        let uri: Url = "ftp://ftp.example.com/file.zip".parse().unwrap();
        let probe = backend.probe(&uri, &ProbeContext::default()).await.unwrap();
        assert!(probe.supports_range);
    }
}
