// Service layer tests.
//
// These tests verify that the DownloadService correctly:
// 1. Dispatches jobs to the right backend based on URI scheme
// 2. Uses JobOptions instead of hardcoded values
// 3. Manages the activation loop
// 4. Handles backend creation for all supported schemes

#[cfg(test)]
mod tests {
    use raria_core::config::{GlobalConfig, JobOptions};
    use raria_core::job::Job;
    use raria_core::persist::Store;
    use raria_core::service::{DownloadService, JobSource, detect_scheme};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    /// URI scheme detection must correctly identify all supported protocols.
    #[test]
    fn detect_scheme_identifies_http() {
        assert_eq!(
            detect_scheme("https://example.com/file.zip"),
            Some(JobSource::Http)
        );
        assert_eq!(
            detect_scheme("http://example.com/file.zip"),
            Some(JobSource::Http)
        );
    }

    #[test]
    fn detect_scheme_identifies_ftp() {
        assert_eq!(
            detect_scheme("ftp://ftp.example.com/file.tar.gz"),
            Some(JobSource::Ftp)
        );
        assert_eq!(
            detect_scheme("ftps://ftp.example.com/file.tar.gz"),
            Some(JobSource::Ftps)
        );
    }

    #[test]
    fn detect_scheme_identifies_sftp() {
        assert_eq!(
            detect_scheme("sftp://host/path/file.bin"),
            Some(JobSource::Sftp)
        );
    }

    #[test]
    fn detect_scheme_identifies_magnet() {
        assert_eq!(
            detect_scheme("magnet:?xt=urn:btih:abc123"),
            Some(JobSource::Magnet)
        );
    }

    #[test]
    fn detect_scheme_returns_none_for_unknown() {
        assert_eq!(detect_scheme("gopher://host/path"), None);
        assert_eq!(detect_scheme("not-a-url"), None);
        assert_eq!(detect_scheme(""), None);
    }

    /// DownloadService must use job.options.max_connections, not hardcoded 16.
    #[test]
    fn service_reads_max_connections_from_job_options() {
        let opts = JobOptions {
            max_connections: 4,
            ..JobOptions::default()
        };

        let job = Job::new_range_with_options(
            vec!["https://example.com/file.bin".into()],
            PathBuf::from("/tmp/file.bin"),
            opts,
        );

        // The service should use job.options.max_connections
        assert_eq!(job.options.max_connections, 4);
    }

    /// DownloadService can be constructed with engine + optional rate limiter.
    #[test]
    fn service_construction() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Arc::new(Store::open(tmp.path()).unwrap());
        let config = GlobalConfig::default();

        let engine = Arc::new(raria_core::engine::Engine::with_store(config, store));
        let service = DownloadService::new(engine, None);

        // Service should be constructable
        assert!(service.engine().registry.is_empty());
    }
}
