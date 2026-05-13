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

            [storage]
            file_allocation = "prealloc"
            conflict_policy = "rename"

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
        assert_eq!(config.storage.file_allocation.as_str(), "prealloc");
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
    fn native_config_converts_to_runtime_global_config() {
        let config = RariaConfig::from_toml_str(
            r#"
            [daemon]
            download_dir = "/downloads"
            session_path = "/state/raria.redb"
            max_active_tasks = 9

            [api]
            listen_addr = "127.0.0.1:7900"

            [downloads]
            default_segments = 7
            min_segment_size = 2097152
            retry_max_attempts = 4

            [network]
            proxy = "http://proxy.example:8080"
            no_proxy = ["localhost"]

            [storage]
            file_allocation = "trunc"
            "#,
        )
        .expect("native config should parse");

        let global = config.to_global_config().expect("convert to global config");

        assert_eq!(global.dir.to_string_lossy(), "/downloads");
        assert_eq!(global.session_file.to_string_lossy(), "/state/raria.redb");
        assert_eq!(global.max_concurrent_downloads, 9);
        assert_eq!(global.rpc_listen_port, 7900);
        assert_eq!(global.split, 7);
        assert_eq!(global.min_split_size, 2097152);
        assert_eq!(global.max_tries, 4);
        assert_eq!(
            global.all_proxy.as_deref(),
            Some("http://proxy.example:8080")
        );
        assert_eq!(global.no_proxy.as_deref(), Some("localhost"));
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
}
