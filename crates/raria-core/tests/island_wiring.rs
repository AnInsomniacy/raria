// Integration tests for island code wiring.
//
// These tests verify that focused runtime modules are wired into native raria
// structures without relying on compatibility parsers.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use std::path::PathBuf;

    /// GlobalConfig should carry load_cookie_file path.
    #[test]
    fn global_config_load_cookie_file_field() {
        let mut config = GlobalConfig::default();
        assert!(config.load_cookie_file.is_none());
        config.load_cookie_file = Some(PathBuf::from("/tmp/cookies.txt"));
        assert_eq!(
            config.load_cookie_file,
            Some(PathBuf::from("/tmp/cookies.txt"))
        );
    }

    /// load_cookie_file serializes/deserializes correctly.
    #[test]
    fn load_cookie_file_serde_roundtrip() {
        let config = GlobalConfig {
            load_cookie_file: Some(PathBuf::from("/tmp/cookies.txt")),
            ..GlobalConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let recovered: GlobalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            recovered.load_cookie_file,
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
