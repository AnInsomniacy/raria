// JobOptions integration tests.
//
// These tests verify that per-job options are properly:
// 1. Stored in the Job struct
// 2. Persisted to/from redb
// 3. Retrievable after restore
// 4. Default to sensible values when not specified
// 5. Override global defaults

#[cfg(test)]
mod tests {
    use raria_core::config::JobOptions;
    use raria_core::job::Job;
    use raria_core::persist::Store;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    /// Job must carry its own options.
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

    /// Job can be created with custom options.
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

    /// Options survive serialization roundtrip (critical for persistence).
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

    /// Options are persisted to redb and recoverable.
    #[test]
    fn job_options_persist_and_recover_via_store() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();

        let opts = JobOptions {
            max_connections: 2,
            out: Some("output.tar.gz".into()),
            ..JobOptions::default()
        };

        let job = Job::new_range_with_options(
            vec!["https://example.com/archive.tar.gz".into()],
            PathBuf::from("/tmp/archive.tar.gz"),
            opts,
        );
        let gid = job.gid;

        // Persist the job (which includes options)
        store.put_job(&job).unwrap();

        // Recover
        let recovered = store.get_job(gid).unwrap().expect("job should exist");
        assert_eq!(recovered.options.max_connections, 2);
        assert_eq!(recovered.options.out.as_deref(), Some("output.tar.gz"));
    }

    /// Default options should produce a Job that works correctly.
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
