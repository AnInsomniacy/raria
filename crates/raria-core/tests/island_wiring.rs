// Integration tests for island code wiring.
//
// These tests verify that focused runtime modules are wired into native raria
// structures without relying on legacy compatibility parsers.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use std::path::PathBuf;

    /// GlobalConfig should carry cookie_file path.
    #[test]
    fn global_config_cookie_file_field() {
        let mut config = GlobalConfig::default();
        assert!(config.cookie_file.is_none());
        config.cookie_file = Some(PathBuf::from("/tmp/cookies.txt"));
        assert_eq!(config.cookie_file, Some(PathBuf::from("/tmp/cookies.txt")));
    }

    /// cookie_file serializes/deserializes correctly.
    #[test]
    fn cookie_file_serde_roundtrip() {
        let config = GlobalConfig {
            cookie_file: Some(PathBuf::from("/tmp/cookies.txt")),
            ..GlobalConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let recovered: GlobalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            recovered.cookie_file,
            Some(PathBuf::from("/tmp/cookies.txt"))
        );
    }

    /// The job source detection must correctly identify magnet URIs as BT.
    #[test]
    fn job_source_detects_magnet() {
        use raria_core::service::{JobSource, detect_scheme};
        let source = detect_scheme("magnet:?xt=urn:btih:abc123");
        assert_eq!(source, Some(JobSource::Magnet));
    }

    /// HTTP URIs are correctly detected.
    #[test]
    fn job_source_detects_http_variants() {
        use raria_core::service::{JobSource, detect_scheme};
        assert_eq!(detect_scheme("http://example.com/f"), Some(JobSource::Http));
        assert_eq!(
            detect_scheme("https://example.com/f"),
            Some(JobSource::Http)
        );
        assert_eq!(
            detect_scheme("ftp://ftp.example.com/f"),
            Some(JobSource::Ftp)
        );
        assert_eq!(detect_scheme("sftp://srv/f"), Some(JobSource::Sftp));
    }
}
