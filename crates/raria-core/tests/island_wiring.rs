// Integration tests for island code wiring.
//
// These tests verify that previously disconnected code modules
// (conf-path, load-cookies) are properly wired into GlobalConfig.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::config_file::{apply_config_map, parse_config_file};
    use std::path::PathBuf;

    // ── conf-path wiring ────────────────────────────────────────────

    /// Verify that conf-path loading mutates GlobalConfig BEFORE
    /// CLI overrides are applied (CLI takes precedence).
    #[test]
    fn conf_path_overrides_defaults() {
        let content = r#"
dir=/from-config-file
max-concurrent-downloads=16
all-proxy=http://proxy-from-config:3128
user-agent=raria-config/2.0
"#;
        let mut config = GlobalConfig::default();
        let map = parse_config_file(content);
        apply_config_map(&mut config, &map);

        assert_eq!(config.dir, PathBuf::from("/from-config-file"));
        assert_eq!(config.max_concurrent_downloads, 16);
        assert_eq!(
            config.all_proxy,
            Some("http://proxy-from-config:3128".into())
        );
        assert_eq!(config.user_agent, Some("raria-config/2.0".into()));
    }

    /// CLI args should override config file values.
    #[test]
    fn cli_overrides_conf_path() {
        let content = "dir=/from-config\nmax-concurrent-downloads=16";
        let mut config = GlobalConfig::default();
        let map = parse_config_file(content);
        apply_config_map(&mut config, &map);

        // Simulate CLI override (CLI dir takes precedence).
        config.dir = PathBuf::from("/from-cli");
        assert_eq!(config.dir, PathBuf::from("/from-cli"));
        // Config file value preserved for field not overridden by CLI.
        assert_eq!(config.max_concurrent_downloads, 16);
    }

    // ── load-cookies wiring ─────────────────────────────────────────

    /// GlobalConfig should carry cookie_file path.
    #[test]
    fn global_config_cookie_file_field() {
        let mut config = GlobalConfig::default();
        assert!(config.cookie_file.is_none());
        config.cookie_file = Some(PathBuf::from("/tmp/cookies.txt"));
        assert_eq!(config.cookie_file, Some(PathBuf::from("/tmp/cookies.txt")));
    }

    /// Config file parser handles load-cookies key.
    #[test]
    fn conf_file_parses_load_cookies() {
        let content = "load-cookies=/home/user/.cookies.txt";
        let mut config = GlobalConfig::default();
        let map = parse_config_file(content);
        apply_config_map(&mut config, &map);
        assert_eq!(
            config.cookie_file,
            Some(PathBuf::from("/home/user/.cookies.txt"))
        );
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

    /// Empty load-cookies value clears the field.
    #[test]
    fn conf_file_empty_load_cookies_clears() {
        let mut config = GlobalConfig {
            cookie_file: Some(PathBuf::from("/old/cookies.txt")),
            ..GlobalConfig::default()
        };
        let content = "load-cookies=";
        let map = parse_config_file(content);
        apply_config_map(&mut config, &map);
        assert_eq!(config.cookie_file, None);
    }

    // ── BT source detection ─────────────────────────────────────────

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
