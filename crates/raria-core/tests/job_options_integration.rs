#[cfg(test)]
mod tests {
    use raria_core::config::JobOptions;
    use raria_core::job::Job;
    use raria_core::native::NativeTaskRow;
    use std::path::PathBuf;

    #[test]
    fn job_has_options_field() {
        let job = Job::new_range(
            vec!["https://example.com/file.bin".into()],
            PathBuf::from("/tmp/file.bin"),
        );

        // Job should have default options embedded
        let opts = &job.options;
        assert_eq!(opts.max_connections, 16);
        assert_eq!(opts.max_download_limit, 0);
    }

    #[test]
    fn job_with_custom_options() {
        let opts = JobOptions {
            max_connections: 4,
            max_download_limit: 1_048_576, // 1 MiB/s
            out: Some("custom_name.zip".into()),
            ..JobOptions::default()
        };

        let job = Job::new_range_with_options(
            vec!["https://example.com/file.bin".into()],
            PathBuf::from("/tmp/file.bin"),
            opts,
        );

        assert_eq!(job.options.max_connections, 4);
        assert_eq!(job.options.max_download_limit, 1_048_576);
        assert_eq!(job.options.out.as_deref(), Some("custom_name.zip"));
    }

    #[test]
    fn job_options_survive_serialization() {
        let mut opts = JobOptions {
            max_connections: 8,
            ..JobOptions::default()
        };
        opts.headers
            .push(("Referer".into(), "https://example.com".into()));

        let job = Job::new_range_with_options(
            vec!["https://example.com/file.bin".into()],
            PathBuf::from("/tmp/file.bin"),
            opts,
        );

        let json = serde_json::to_string(&job).unwrap();
        let recovered: Job = serde_json::from_str(&json).unwrap();

        assert_eq!(recovered.options.max_connections, 8);
        assert_eq!(recovered.options.headers.len(), 1);
        assert_eq!(recovered.options.headers[0].0, "Referer");
    }

    #[test]
    fn native_task_row_carries_persisted_task_policy() {
        let opts = JobOptions {
            max_connections: 2,
            max_download_limit: 1024,
            out: Some("output.tar.gz".into()),
            ..JobOptions::default()
        };

        let job = Job::new_range_with_options(
            vec!["https://example.com/archive.tar.gz".into()],
            PathBuf::from("/tmp/archive.tar.gz"),
            opts,
        );
        let row = NativeTaskRow::from_runtime_job(&job);
        let json = serde_json::to_string(&row).unwrap();
        let recovered: NativeTaskRow = serde_json::from_str(&json).unwrap();

        assert_eq!(recovered.segments, 2);
        assert_eq!(recovered.output_path, PathBuf::from("/tmp/archive.tar.gz"));
    }

    #[test]
    fn default_options_are_production_ready() {
        let opts = JobOptions::default();

        assert!(opts.max_connections > 0, "max_connections must be positive");
        assert!(
            opts.max_connections <= 16,
            "max_connections should not exceed 16 by default"
        );
        assert_eq!(opts.max_download_limit, 0, "default should be unlimited");
        assert_eq!(opts.max_upload_limit, 0, "default should be unlimited");
        assert!(opts.headers.is_empty(), "no headers by default");
        assert!(opts.dir.is_none(), "no dir override by default");
        assert!(opts.out.is_none(), "no filename override by default");
    }
}
