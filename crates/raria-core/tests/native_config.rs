#[cfg(test)]
mod tests {
    use raria_core::native_config::RariaConfig;

    #[test]
    fn raria_toml_loads_native_sections() {
        let config = RariaConfig::from_toml_str(
            r#"
            [daemon]
            download_dir = "/downloads"
            session_path = "/state/raria.redb"
            max_active_tasks = 8

            [api]
            listen_addr = "127.0.0.1:7800"
            allow_origins = ["https://ui.example"]

            [downloads]
            default_segments = 6
            min_segment_size = 1048576
            retry_max_attempts = 7

            [network]
            proxy = "socks5://127.0.0.1:1080"
            no_proxy = ["localhost", "127.0.0.1"]

            [bittorrent]
            enable_dht = true
            enable_udp_trackers = true
            enable_pex = true
            seed_ratio = 1.5
            seed_time = 60

            [metalink]
            preferred_locations = ["jp", "us"]
            preferred_protocol = "https"
            unique_protocols = true

            [storage]
            file_allocation = "prealloc"
            conflict_policy = "rename"

            [hooks]
            task_started = "/hooks/task-started.sh"
            task_completed = "/hooks/task-completed.sh"
            task_failed = "/hooks/task-failed.sh"

            [logging]
            structured_log_path = "/logs/raria.jsonl"
            "#,
        )
        .expect("native config should parse");

        assert_eq!(config.daemon.max_active_tasks, 8);
        assert_eq!(config.api.listen_addr, "127.0.0.1:7800");
        assert_eq!(config.downloads.default_segments, 6);
        assert_eq!(config.network.no_proxy, vec!["localhost", "127.0.0.1"]);
        assert!(config.bittorrent.enable_dht);
        assert_eq!(config.metalink.preferred_locations, vec!["jp", "us"]);
        assert_eq!(config.metalink.preferred_protocol.as_deref(), Some("https"));
        assert!(config.metalink.unique_protocols);
        assert_eq!(config.storage.file_allocation.as_str(), "prealloc");
        assert_eq!(
            config.hooks.task_started.as_deref(),
            Some(std::path::Path::new("/hooks/task-started.sh"))
        );
        assert_eq!(
            config.hooks.task_completed.as_deref(),
            Some(std::path::Path::new("/hooks/task-completed.sh"))
        );
        assert_eq!(
            config.hooks.task_failed.as_deref(),
            Some(std::path::Path::new("/hooks/task-failed.sh"))
        );
    }

    #[test]
    fn raria_toml_rejects_unknown_fields() {
        let err = RariaConfig::from_toml_str(
            r#"
            [daemon]
            download_dir = "/downloads"
            legacy_key = true
            "#,
        )
        .expect_err("unknown fields must fail");

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn raria_toml_rejects_legacy_aria2_names() {
        let err = RariaConfig::from_toml_str(
            r#"
            [daemon]
            dir = "/downloads"
            rpc_secret = "secret"
            "#,
        )
        .expect_err("legacy names must fail");

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn raria_toml_rejects_legacy_hook_names() {
        let err = RariaConfig::from_toml_str(
            r#"
            [hooks]
            on-download-start = "/tmp/start.sh"
            "#,
        )
        .expect_err("legacy hook names must fail");

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn native_config_converts_to_runtime_global_config() {
        let config = RariaConfig::from_toml_str(
            r#"
            [daemon]
            download_dir = "/downloads"
            session_path = "/state/raria.redb"
            max_active_tasks = 9
            stop_after_seconds = 60
            stop_when_parent_exits = 12345

            [api]
            listen_addr = "127.0.0.1:7900"

            [downloads]
            default_segments = 7
            min_segment_size = 2097152
            retry_max_attempts = 4

            [network]
            proxy = "http://proxy.example:8080"
            no_proxy = ["localhost"]

            [metalink]
            preferred_locations = ["de"]
            preferred_protocol = "ftp"
            unique_protocols = true

            [storage]
            file_allocation = "trunc"
            conflict_policy = "overwrite"

            [hooks]
            task_started = "/hooks/start.sh"
            task_completed = "/hooks/complete.sh"
            task_failed = "/hooks/fail.sh"
            "#,
        )
        .expect("native config should parse");

        let global = config.to_global_config().expect("convert to global config");

        assert_eq!(global.download_dir.to_string_lossy(), "/downloads");
        assert_eq!(global.session_file.to_string_lossy(), "/state/raria.redb");
        assert_eq!(global.max_concurrent_downloads, 9);
        assert_eq!(global.daemon_stop_after_seconds, Some(60));
        assert_eq!(global.daemon_parent_pid, Some(12345));
        assert_eq!(global.api_listen_port, 7900);
        assert_eq!(global.default_segments, 7);
        assert_eq!(global.min_segment_size, 2097152);
        assert_eq!(global.retry_attempts, 4);
        assert!(global.bt_enable_pex);
        assert_eq!(global.proxy.as_deref(), Some("http://proxy.example:8080"));
        assert_eq!(global.no_proxy.as_deref(), Some("localhost"));
        assert_eq!(global.metalink_preferred_locations, vec!["de"]);
        assert_eq!(global.metalink_preferred_protocol.as_deref(), Some("ftp"));
        assert!(global.metalink_unique_protocols);
        assert!(!global.auto_file_renaming);
        assert!(global.allow_overwrite);
        assert_eq!(
            global.on_task_start.as_deref(),
            Some(std::path::Path::new("/hooks/start.sh"))
        );
        assert_eq!(
            global.on_task_complete.as_deref(),
            Some(std::path::Path::new("/hooks/complete.sh"))
        );
        assert_eq!(
            global.on_task_fail.as_deref(),
            Some(std::path::Path::new("/hooks/fail.sh"))
        );
    }

    #[test]
    fn native_config_loads_api_token_from_file() {
        let temp = tempfile::NamedTempFile::new().expect("token file");
        std::fs::write(temp.path(), "secret-token\n").expect("write token");

        let config = RariaConfig::from_toml_str(&format!(
            r#"
            [api]
            auth_token_file = "{}"
            "#,
            temp.path().display()
        ))
        .expect("native config should parse");

        assert_eq!(
            config.api_auth_token().expect("token").as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn native_config_carries_api_token_into_runtime_config() {
        let temp = tempfile::NamedTempFile::new().expect("token file");
        std::fs::write(temp.path(), "runtime-token\n").expect("write token");

        let config = RariaConfig::from_toml_str(&format!(
            r#"
            [api]
            auth_token_file = "{}"
            "#,
            temp.path().display()
        ))
        .expect("native config should parse");

        let global = config.to_global_config().expect("convert to global config");

        assert_eq!(global.api_auth_token.as_deref(), Some("runtime-token"));
    }

    #[test]
    fn native_config_carries_pex_policy_into_runtime_config() {
        let config = RariaConfig::from_toml_str(
            r#"
            [bittorrent]
            enable_pex = false
            "#,
        )
        .expect("native config should parse");

        let global = config.to_global_config().expect("convert to global config");

        assert!(!global.bt_enable_pex);
    }
}
