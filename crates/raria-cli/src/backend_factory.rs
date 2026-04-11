use anyhow::Result;
use raria_core::service::{JobSource, detect_scheme};
use raria_http::backend::HttpBackend;
use raria_range::backend::ByteSourceBackend;
use std::sync::Arc;

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn create_backend(uri: &str) -> Result<Arc<dyn ByteSourceBackend>> {
    create_backend_with_config(uri, None, None, None)
}

pub(crate) fn create_backend_with_config(
    uri: &str,
    http_config: Option<&raria_http::backend::HttpBackendConfig>,
    ftp_config: Option<&raria_ftp::backend::FtpBackendConfig>,
    sftp_config: Option<&raria_sftp::backend::SftpBackendConfig>,
) -> Result<Arc<dyn ByteSourceBackend>> {
    use raria_ftp::backend::FtpBackend;
    use raria_sftp::backend::SftpBackend;

    let source = detect_scheme(uri)
        .ok_or_else(|| anyhow::anyhow!("unsupported or unrecognized URI scheme: {uri}"))?;

    match source {
        JobSource::Http => {
            if let Some(config) = http_config {
                Ok(Arc::new(HttpBackend::with_config(config)?))
            } else {
                Ok(Arc::new(HttpBackend::new()?))
            }
        }
        JobSource::Ftp | JobSource::Ftps => {
            if let Some(config) = ftp_config {
                Ok(Arc::new(FtpBackend::with_config(config.clone())))
            } else {
                Ok(Arc::new(FtpBackend::new()))
            }
        }
        JobSource::Sftp => {
            if let Some(config) = sftp_config {
                Ok(Arc::new(SftpBackend::with_config(config.clone())))
            } else {
                Ok(Arc::new(SftpBackend::new()))
            }
        }
        JobSource::Magnet => Err(anyhow::anyhow!(
            "magnet URIs use BitTorrent, not range-based download"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::create_backend;

    #[test]
    fn dispatch_https_to_http_backend() {
        let backend = create_backend("https://example.com/file.zip").unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn dispatch_http_to_http_backend() {
        let backend = create_backend("http://example.com/file.zip").unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn dispatch_ftp_to_ftp_backend() {
        let backend = create_backend("ftp://ftp.example.com/pub/file.tar.gz").unwrap();
        assert_eq!(backend.name(), "ftp");
    }

    #[test]
    fn dispatch_ftps_to_ftp_backend() {
        let backend = create_backend("ftps://ftp.example.com/secure/file.zip").unwrap();
        assert_eq!(backend.name(), "ftp");
    }

    #[test]
    fn dispatch_sftp_to_sftp_backend() {
        let backend = create_backend("sftp://server.example.com/home/user/file.bin").unwrap();
        assert_eq!(backend.name(), "sftp");
    }

    #[test]
    fn dispatch_unknown_scheme_errors() {
        let result = create_backend("gopher://old.server.net/file");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported"));
    }

    #[test]
    fn dispatch_empty_uri_errors() {
        assert!(create_backend("").is_err());
    }

    #[test]
    fn dispatch_magnet_errors_for_range_backend() {
        let result = create_backend("magnet:?xt=urn:btih:abc123");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("magnet"));
    }

    #[test]
    fn dispatch_ftp_with_credentials() {
        let backend = create_backend("ftp://user:pass@ftp.example.com/file.zip").unwrap();
        assert_eq!(backend.name(), "ftp");
    }

    #[test]
    fn dispatch_http_custom_port() {
        let backend = create_backend("http://example.com:8080/file.zip").unwrap();
        assert_eq!(backend.name(), "http");
    }
}
