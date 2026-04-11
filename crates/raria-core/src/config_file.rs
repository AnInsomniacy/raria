// raria-core: Configuration file parser.
//
// Parses aria2-compatible configuration files. Format:
// - Lines starting with # are comments.
// - Options use key=value format (same as CLI without --)
// - Empty lines are skipped.
//
// Example:
//   dir=/home/user/downloads
//   max-concurrent-downloads=5
//   max-overall-download-limit=1048576
//   all-proxy=http://proxy:8080

use crate::config::GlobalConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parse a configuration file and return a map of key-value pairs.
///
/// This is the low-level parser that doesn't know about GlobalConfig.
/// Invalid lines are silently ignored (matches aria2 behavior).
pub fn parse_config_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        // Skip empty lines and comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Split on first '='.
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            if !key.is_empty() {
                map.insert(key.to_string(), value.to_string());
            }
        }
    }
    map
}

/// Controls how config parsing errors are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigParseMode {
    /// Log warnings for invalid values but continue (aria2 compatibility).
    Lenient,
    /// Return an error on the first invalid value (fail-fast for daemon mode).
    Strict,
}

/// Apply parsed key-value options onto a GlobalConfig.
///
/// Unknown keys are silently ignored (forward compatibility).
/// Invalid values for known keys are silently skipped (aria2 behavior).
pub fn apply_config_map(config: &mut GlobalConfig, map: &HashMap<String, String>) {
    // Delegate to lenient mode, ignore the always-Ok result.
    let _ = apply_config_map_with_mode(config, map, ConfigParseMode::Lenient);
}

/// Apply parsed key-value options onto a GlobalConfig with explicit error handling.
///
/// - [`ConfigParseMode::Lenient`]: invalid values are silently skipped.
/// - [`ConfigParseMode::Strict`]: returns `Err` with the key name on the first
///   value that cannot be parsed.
pub fn apply_config_map_with_mode(
    config: &mut GlobalConfig,
    map: &HashMap<String, String>,
    mode: ConfigParseMode,
) -> anyhow::Result<()> {
    /// Helper: parse an integer value, returning an error in strict mode.
    macro_rules! parse_int {
        ($key:expr, $value:expr, $field:expr, $mode:expr) => {
            match $value.parse() {
                Ok(n) => $field = n,
                Err(_) if $mode == ConfigParseMode::Strict => {
                    anyhow::bail!("invalid value for '{}': expected integer, got '{}'", $key, $value);
                }
                Err(_) => {} // lenient: skip
            }
        };
    }

    for (key, value) in map {
        match key.as_str() {
            "dir" => config.dir = PathBuf::from(value),
            "max-concurrent-downloads" => {
                parse_int!(key, value, config.max_concurrent_downloads, mode);
            }
            "max-overall-download-limit" => {
                parse_int!(key, value, config.max_overall_download_limit, mode);
            }
            "max-overall-upload-limit" => {
                parse_int!(key, value, config.max_overall_upload_limit, mode);
            }
            "rpc-listen-port" => {
                parse_int!(key, value, config.rpc_listen_port, mode);
            }
            "enable-rpc" | "rpc" => {
                config.enable_rpc = value == "true" || value == "1";
            }
            "log-level" => {
                config.log_level = value.clone();
            }
            "quiet" => {
                config.quiet = value == "true" || value == "1";
            }
            "all-proxy" => {
                config.all_proxy = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "http-proxy" => {
                config.http_proxy = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "https-proxy" => {
                config.https_proxy = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "no-proxy" => {
                config.no_proxy = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "check-certificate" => {
                config.check_certificate = value == "true" || value == "1";
            }
            "ca-certificate" => {
                config.ca_certificate = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "certificate" => {
                config.certificate = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "private-key" => {
                config.private_key = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "user-agent" => {
                config.user_agent = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "http-user" => {
                config.http_user = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "http-passwd" => {
                config.http_passwd = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "load-cookies" => {
                config.cookie_file = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "save-cookies" => {
                config.save_cookie_file = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "bt-dht-config-file" => {
                config.bt_dht_config_file = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "rpc-secret" => {
                config.rpc_secret = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "save-session-interval" => {
                match value.parse::<u64>() {
                    Ok(n) => config.save_session_interval = Some(n),
                    Err(_) if mode == ConfigParseMode::Strict => {
                        anyhow::bail!("invalid value for '{}': expected integer, got '{}'", key, value);
                    }
                    Err(_) => config.save_session_interval = None,
                }
            }
            "rpc-allow-origin-all" => {
                config.rpc_allow_origin_all = value == "true" || value == "1";
            }
            "file-allocation" => {
                match crate::file_alloc::FileAllocation::parse(value) {
                    Ok(m) => config.file_allocation = m,
                    Err(_) if mode == ConfigParseMode::Strict => {
                        anyhow::bail!("invalid value for '{}': unrecognized mode '{}'", key, value);
                    }
                    Err(_) => {} // lenient: skip
                }
            }
            "max-connection-per-server" => {
                parse_int!(key, value, config.max_connection_per_server, mode);
            }
            "split" => {
                parse_int!(key, value, config.split, mode);
            }
            "continue" => {
                config.continue_download = value == "true" || value.is_empty();
            }
            "min-split-size" => {
                parse_int!(key, value, config.min_split_size, mode);
            }
            "lowest-speed-limit" => {
                parse_int!(key, value, config.lowest_speed_limit, mode);
            }
            "max-file-not-found" => {
                parse_int!(key, value, config.max_file_not_found, mode);
            }
            "max-tries" => {
                parse_int!(key, value, config.max_tries, mode);
            }
            "retry-wait" => {
                parse_int!(key, value, config.retry_wait, mode);
            }
            "max-redirect" => {
                match value.parse::<usize>() {
                    Ok(n) => config.max_redirects = Some(n),
                    Err(_) if mode == ConfigParseMode::Strict => {
                        anyhow::bail!("invalid value for '{}': expected integer, got '{}'", key, value);
                    }
                    Err(_) => config.max_redirects = None,
                }
            }
            "netrc-path" => {
                config.netrc_path = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "no-netrc" => {
                config.no_netrc = value == "true" || value == "1";
            }
            "timeout" => {
                match value.parse::<u64>() {
                    Ok(n) => config.timeout = Some(n),
                    Err(_) if mode == ConfigParseMode::Strict => {
                        anyhow::bail!("invalid value for '{}': expected integer, got '{}'", key, value);
                    }
                    Err(_) => config.timeout = None,
                }
            }
            "connect-timeout" => {
                match value.parse::<u64>() {
                    Ok(n) => config.connect_timeout = Some(n),
                    Err(_) if mode == ConfigParseMode::Strict => {
                        anyhow::bail!("invalid value for '{}': expected integer, got '{}'", key, value);
                    }
                    Err(_) => config.connect_timeout = None,
                }
            }
            "conditional-get" => {
                config.conditional_get = value == "true" || value == "1";
            }
            "allow-overwrite" => {
                config.allow_overwrite = value == "true" || value == "1";
            }
            "sftp-strict-host-key-check" => {
                config.sftp_strict_host_key_check = value == "true" || value == "1";
            }
            "sftp-known-hosts" => {
                config.sftp_known_hosts = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "sftp-private-key" => {
                config.sftp_private_key = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "sftp-private-key-passphrase" => {
                config.sftp_private_key_passphrase = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
            }
            "on-download-start" => {
                config.on_download_start = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "on-download-complete" => {
                config.on_download_complete = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "on-download-error" => {
                config.on_download_error = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "auto-file-renaming" => {
                config.auto_file_renaming = value == "true" || value == "1";
            }
            _ => {
                // Unknown key — silently ignore for forward compatibility.
            }
        }
    }
    Ok(())
}

/// Load and apply a configuration file onto an existing GlobalConfig.
///
/// Returns `Ok(())` if the file was loaded, `Err` if the file couldn't be read.
/// If the file doesn't exist, returns `Ok(())` silently (no error).
pub fn load_config_file(config: &mut GlobalConfig, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path)?;
    let map = parse_config_file(&content);
    apply_config_map(config, &map);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_content() {
        let map = parse_config_file("");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_comments_and_blank_lines() {
        let content = r#"
# This is a comment
   # Another comment

# Blank line above
"#;
        let map = parse_config_file(content);
        assert!(map.is_empty());
    }

    #[test]
    fn parse_basic_key_value() {
        let content = "dir=/home/user/downloads\nmax-concurrent-downloads=5";
        let map = parse_config_file(content);
        assert_eq!(map.get("dir").unwrap(), "/home/user/downloads");
        assert_eq!(map.get("max-concurrent-downloads").unwrap(), "5");
    }

    #[test]
    fn parse_value_with_equals_sign() {
        // Value might contain '=' (e.g., proxy URL with query param)
        let content = "all-proxy=http://proxy:8080?auth=key123";
        let map = parse_config_file(content);
        assert_eq!(
            map.get("all-proxy").unwrap(),
            "http://proxy:8080?auth=key123"
        );
    }

    #[test]
    fn parse_trims_whitespace() {
        let content = "  dir  =  /tmp/downloads  \n  log-level = debug  ";
        let map = parse_config_file(content);
        assert_eq!(map.get("dir").unwrap(), "/tmp/downloads");
        assert_eq!(map.get("log-level").unwrap(), "debug");
    }

    #[test]
    fn apply_config_map_sets_rpc_allow_origin_all() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("rpc-allow-origin-all".into(), "true".into());

        apply_config_map(&mut config, &map);

        assert!(config.rpc_allow_origin_all);
    }

    #[test]
    fn apply_config_map_clears_rpc_allow_origin_all_with_false() {
        let mut config = GlobalConfig {
            rpc_allow_origin_all: true,
            ..GlobalConfig::default()
        };
        let mut map = HashMap::new();
        map.insert("rpc-allow-origin-all".into(), "false".into());

        apply_config_map(&mut config, &map);

        assert!(!config.rpc_allow_origin_all);
    }

    #[test]
    fn parse_empty_value() {
        let content = "all-proxy=";
        let map = parse_config_file(content);
        assert_eq!(map.get("all-proxy").unwrap(), "");
    }

    #[test]
    fn apply_config_dir() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("dir".into(), "/custom/path".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.dir, PathBuf::from("/custom/path"));
    }

    #[test]
    fn apply_config_concurrent() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("max-concurrent-downloads".into(), "10".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.max_concurrent_downloads, 10);
    }

    #[test]
    fn apply_config_min_split_size() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("min-split-size".into(), "262144".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.min_split_size, 262144);
    }

    #[test]
    fn apply_config_lowest_speed_limit() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("lowest-speed-limit".into(), "1024".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.lowest_speed_limit, 1024);
    }

    #[test]
    fn apply_config_max_file_not_found() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("max-file-not-found".into(), "2".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.max_file_not_found, 2);
    }

    #[test]
    fn apply_config_save_cookies() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("save-cookies".into(), "/tmp/cookies.txt".into());
        apply_config_map(&mut config, &map);
        assert_eq!(
            config.save_cookie_file,
            Some(PathBuf::from("/tmp/cookies.txt"))
        );
    }

    #[test]
    fn apply_config_hook_scripts() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("on-download-start".into(), "/tmp/start.sh".into());
        map.insert("on-download-complete".into(), "/tmp/complete.sh".into());
        map.insert("on-download-error".into(), "/tmp/error.sh".into());
        apply_config_map(&mut config, &map);
        assert_eq!(
            config.on_download_start,
            Some(PathBuf::from("/tmp/start.sh"))
        );
        assert_eq!(
            config.on_download_complete,
            Some(PathBuf::from("/tmp/complete.sh"))
        );
        assert_eq!(
            config.on_download_error,
            Some(PathBuf::from("/tmp/error.sh"))
        );
    }

    #[test]
    fn apply_config_proxy() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("all-proxy".into(), "http://proxy:8080".into());
        map.insert("no-proxy".into(), "localhost,127.0.0.1".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.all_proxy, Some("http://proxy:8080".into()));
        assert_eq!(config.no_proxy, Some("localhost,127.0.0.1".into()));
    }

    #[test]
    fn apply_config_tls() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("check-certificate".into(), "false".into());
        map.insert("ca-certificate".into(), "/etc/ssl/ca.pem".into());
        map.insert("certificate".into(), "/etc/ssl/client.pem".into());
        map.insert("private-key".into(), "/etc/ssl/client.key".into());
        apply_config_map(&mut config, &map);
        assert!(!config.check_certificate);
        assert_eq!(
            config.ca_certificate,
            Some(PathBuf::from("/etc/ssl/ca.pem"))
        );
        assert_eq!(
            config.certificate,
            Some(PathBuf::from("/etc/ssl/client.pem"))
        );
        assert_eq!(
            config.private_key,
            Some(PathBuf::from("/etc/ssl/client.key"))
        );
    }

    #[test]
    fn apply_config_http_basic_auth() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("http-user".into(), "cfg-user".into());
        map.insert("http-passwd".into(), "cfg-pass".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.http_user.as_deref(), Some("cfg-user"));
        assert_eq!(config.http_passwd.as_deref(), Some("cfg-pass"));
    }

    #[test]
    fn apply_config_empty_proxy_clears_it() {
        let mut config = GlobalConfig {
            all_proxy: Some("http://old-proxy".into()),
            ..GlobalConfig::default()
        };
        let mut map = HashMap::new();
        map.insert("all-proxy".into(), "".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.all_proxy, None);
    }

    #[test]
    fn apply_config_unknown_keys_ignored() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("unknown-key".into(), "some-value".into());
        map.insert("another-unknown".into(), "data".into());
        // Should not panic or error.
        apply_config_map(&mut config, &map);
    }

    #[test]
    fn apply_config_rpc_enable_variants() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("enable-rpc".into(), "true".into());
        apply_config_map(&mut config, &map);
        assert!(config.enable_rpc);

        config.enable_rpc = false;
        map.insert("enable-rpc".into(), "1".into());
        apply_config_map(&mut config, &map);
        assert!(config.enable_rpc);
    }

    #[test]
    fn apply_config_quiet() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("quiet".into(), "true".into());
        apply_config_map(&mut config, &map);
        assert!(config.quiet);
    }

    #[test]
    fn apply_config_speed_limits() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("max-overall-download-limit".into(), "1048576".into());
        map.insert("max-overall-upload-limit".into(), "524288".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.max_overall_download_limit, 1048576);
        assert_eq!(config.max_overall_upload_limit, 524288);
    }

    #[test]
    fn apply_config_invalid_number_ignored() {
        let mut config = GlobalConfig::default();
        let original_concurrent = config.max_concurrent_downloads;
        let mut map = HashMap::new();
        map.insert("max-concurrent-downloads".into(), "not-a-number".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.max_concurrent_downloads, original_concurrent);
    }

    #[test]
    fn load_nonexistent_file_returns_ok() {
        let mut config = GlobalConfig::default();
        let result = load_config_file(&mut config, Path::new("/nonexistent/path/config"));
        assert!(result.is_ok());
    }

    #[test]
    fn full_config_file_roundtrip() {
        let content = r#"
# raria configuration
dir=/home/user/downloads
max-concurrent-downloads=8
max-overall-download-limit=0
rpc-listen-port=6800
enable-rpc=true
log-level=info
all-proxy=http://proxy:3128
check-certificate=true
user-agent=raria/1.0
"#;
        let mut config = GlobalConfig::default();
        let map = parse_config_file(content);
        apply_config_map(&mut config, &map);

        assert_eq!(config.dir, PathBuf::from("/home/user/downloads"));
        assert_eq!(config.max_concurrent_downloads, 8);
        assert_eq!(config.max_overall_download_limit, 0);
        assert_eq!(config.rpc_listen_port, 6800);
        assert!(config.enable_rpc);
        assert_eq!(config.log_level, "info");
        assert_eq!(config.all_proxy, Some("http://proxy:3128".into()));
        assert!(config.check_certificate);
        assert_eq!(config.user_agent, Some("raria/1.0".into()));
    }

    #[test]
    fn apply_config_max_redirects() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("max-redirect".into(), "3".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.max_redirects, Some(3));
    }

    #[test]
    fn apply_config_netrc_path() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("netrc-path".into(), "/tmp/test.netrc".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.netrc_path, Some(PathBuf::from("/tmp/test.netrc")));
    }

    #[test]
    fn apply_config_no_netrc_true() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("no-netrc".into(), "true".into());
        apply_config_map(&mut config, &map);
        assert!(config.no_netrc);
    }

    #[test]
    fn apply_config_timeout() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("timeout".into(), "12".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.timeout, Some(12));
    }

    #[test]
    fn apply_config_connect_timeout() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("connect-timeout".into(), "7".into());
        apply_config_map(&mut config, &map);
        assert_eq!(config.connect_timeout, Some(7));
    }

    #[test]
    fn apply_config_conditional_get() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("conditional-get".into(), "true".into());
        apply_config_map(&mut config, &map);
        assert!(config.conditional_get);
    }

    #[test]
    fn apply_config_allow_overwrite() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("allow-overwrite".into(), "true".into());
        apply_config_map(&mut config, &map);
        assert!(config.allow_overwrite);
    }

    #[test]
    fn apply_config_sftp_strict_host_key_check() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("sftp-strict-host-key-check".into(), "true".into());
        apply_config_map(&mut config, &map);
        assert!(config.sftp_strict_host_key_check);
    }

    #[test]
    fn apply_config_sftp_known_hosts() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("sftp-known-hosts".into(), "/tmp/known_hosts".into());
        apply_config_map(&mut config, &map);
        assert_eq!(
            config.sftp_known_hosts,
            Some(PathBuf::from("/tmp/known_hosts"))
        );
    }

    #[test]
    fn apply_config_sftp_private_key() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("sftp-private-key".into(), "/tmp/id_ed25519".into());
        map.insert("sftp-private-key-passphrase".into(), "secret".into());
        apply_config_map(&mut config, &map);
        assert_eq!(
            config.sftp_private_key,
            Some(PathBuf::from("/tmp/id_ed25519"))
        );
        assert_eq!(
            config.sftp_private_key_passphrase.as_deref(),
            Some("secret")
        );
    }

    #[test]
    fn apply_config_bt_dht_config_file() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("bt-dht-config-file".into(), "/tmp/raria-dht.json".into());
        apply_config_map(&mut config, &map);
        assert_eq!(
            config.bt_dht_config_file,
            Some(PathBuf::from("/tmp/raria-dht.json"))
        );
    }

    #[test]
    fn apply_config_auto_file_renaming_false() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("auto-file-renaming".into(), "false".into());
        apply_config_map(&mut config, &map);
        assert!(!config.auto_file_renaming);
    }
    #[test]
    fn strict_mode_rejects_invalid_integer_value() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("max-concurrent-downloads".into(), "not_a_number".into());
        let result = apply_config_map_with_mode(&mut config, &map, ConfigParseMode::Strict);
        assert!(result.is_err(), "strict mode must reject unparseable integer");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("max-concurrent-downloads"),
            "error must name the invalid key: {err_msg}"
        );
    }

    #[test]
    fn strict_mode_accepts_valid_config() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("max-concurrent-downloads".into(), "10".into());
        map.insert("dir".into(), "/tmp/downloads".into());
        let result = apply_config_map_with_mode(&mut config, &map, ConfigParseMode::Strict);
        assert!(result.is_ok(), "strict mode must accept valid config");
        assert_eq!(config.max_concurrent_downloads, 10);
    }

    #[test]
    fn lenient_mode_ignores_invalid_integer_value() {
        let mut config = GlobalConfig::default();
        let original_value = config.max_concurrent_downloads;
        let mut map = HashMap::new();
        map.insert("max-concurrent-downloads".into(), "not_a_number".into());
        let result = apply_config_map_with_mode(&mut config, &map, ConfigParseMode::Lenient);
        assert!(result.is_ok(), "lenient mode must not error");
        assert_eq!(
            config.max_concurrent_downloads, original_value,
            "invalid value should not change config"
        );
    }

    #[test]
    fn strict_mode_rejects_invalid_file_allocation_value() {
        let mut config = GlobalConfig::default();
        let mut map = HashMap::new();
        map.insert("file-allocation".into(), "invalid_mode".into());
        let result = apply_config_map_with_mode(&mut config, &map, ConfigParseMode::Strict);
        assert!(result.is_err(), "strict mode must reject invalid file-allocation");
    }
}
