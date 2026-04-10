// RPC response format parity tests.
//
// These tests verify that raria's RPC responses match aria2 1.37.0's exact
// field names, types, and value formats. Critical for AriaNg/Motrix compatibility.
//
// aria2 contract (from manual):
// - All numeric values are strings: "1024", not 1024
// - GIDs are 16-char lowercase hex strings
// - Status values: "active", "waiting", "paused", "complete", "error", "removed"
// - camelCase field names: totalLength, completedLength, downloadSpeed, etc.
// - Files array uses string index starting from "1"
// - URI status is "used" or "waiting"

#[cfg(test)]
mod tests {
    use raria_core::job::{Gid, Job, Status};
    use raria_rpc::facade::{compute_global_stat, job_to_aria2_status};
    use std::path::PathBuf;

    // ═══════════════════════════════════════════════════════════════════
    // tellStatus response parity
    // ═══════════════════════════════════════════════════════════════════

    /// All numeric fields in tellStatus response must be strings, not numbers.
    /// This is the #1 source of aria2 client breakage.
    #[test]
    fn tell_status_all_numeric_fields_are_strings() {
        let mut job = Job::new_range(
            vec!["https://example.com/file.bin".into()],
            PathBuf::from("/tmp/downloads/file.bin"),
        );
        job.total_size = Some(1_048_576);
        job.downloaded = 524_288;
        job.download_speed = 102_400;
        job.upload_speed = 0;

        let status = job_to_aria2_status(&job);
        let json = serde_json::to_value(&status).unwrap();

        // All these must be strings
        assert!(
            json["totalLength"].is_string(),
            "totalLength must be a string"
        );
        assert!(
            json["completedLength"].is_string(),
            "completedLength must be a string"
        );
        assert!(
            json["downloadSpeed"].is_string(),
            "downloadSpeed must be a string"
        );
        assert!(
            json["uploadSpeed"].is_string(),
            "uploadSpeed must be a string"
        );
        assert!(
            json["connections"].is_string(),
            "connections must be a string"
        );

        // And they must contain the correct numeric values as strings
        assert_eq!(json["totalLength"], "1048576");
        assert_eq!(json["completedLength"], "524288");
        assert_eq!(json["downloadSpeed"], "102400");
        assert_eq!(json["uploadSpeed"], "0");
    }

    /// Status field must use exact aria2 string values.
    #[test]
    fn tell_status_status_field_exact_values() {
        let cases = vec![
            (Status::Active, "active"),
            (Status::Waiting, "waiting"),
            (Status::Paused, "paused"),
            (Status::Complete, "complete"),
            (Status::Error, "error"),
            (Status::Removed, "removed"),
        ];

        for (status, expected) in cases {
            let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
            job.status = status;
            let result = job_to_aria2_status(&job);
            assert_eq!(
                result.status, expected,
                "Status::{:?} should map to \"{}\"",
                status, expected
            );
        }
    }

    /// GID in status response must be 16-char zero-padded lowercase hex.
    #[test]
    fn tell_status_gid_format() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.gid = Gid::from_raw(0xDEADBEEF);

        let status = job_to_aria2_status(&job);
        assert_eq!(status.gid, "00000000deadbeef");
        assert_eq!(status.gid.len(), 16);
    }

    /// Files array must exist with string-typed fields.
    #[test]
    fn tell_status_files_array_format() {
        let job = Job::new_range(
            vec![
                "https://mirror1.com/f".into(),
                "https://mirror2.com/f".into(),
            ],
            PathBuf::from("/tmp/downloads/f.zip"),
        );
        let status = job_to_aria2_status(&job);
        let json = serde_json::to_value(&status).unwrap();

        // files must be an array
        assert!(json["files"].is_array());
        let files = json["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);

        // File index must be string "1" (aria2 is 1-based)
        assert_eq!(files[0]["index"], "1");

        // File length and completedLength must be strings
        assert!(files[0]["length"].is_string());
        assert!(files[0]["completedLength"].is_string());

        // selected must be string "true"
        assert_eq!(files[0]["selected"], "true");

        // URIs must be present
        let uris = files[0]["uris"].as_array().unwrap();
        assert_eq!(uris.len(), 2);
        assert_eq!(uris[0]["uri"], "https://mirror1.com/f");
        assert!(
            uris[0]["status"] == "used" || uris[0]["status"] == "waiting",
            "URI status must be 'used' or 'waiting'"
        );
    }

    /// Error status must include errorCode and errorMessage.
    #[test]
    fn tell_status_error_has_code_and_message() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.status = Status::Error;
        job.error_msg = Some("connection refused".into());

        let status = job_to_aria2_status(&job);
        let json = serde_json::to_value(&status).unwrap();

        assert_eq!(json["errorCode"], "1");
        assert_eq!(json["errorMessage"], "connection refused");
    }

    /// Non-error status must NOT include errorCode/errorMessage.
    #[test]
    fn tell_status_non_error_has_no_error_fields() {
        let job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        let json = serde_json::to_value(job_to_aria2_status(&job)).unwrap();

        assert!(json.get("errorCode").is_none() || json["errorCode"].is_null());
        assert!(json.get("errorMessage").is_none() || json["errorMessage"].is_null());
    }

    /// BT jobs must have a "bittorrent" field; range jobs must not.
    #[test]
    fn tell_status_bittorrent_field_presence() {
        let range_job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        let bt_job = Job::new_bt(vec![], PathBuf::from("/tmp/f"));

        let range_json = serde_json::to_value(job_to_aria2_status(&range_job)).unwrap();
        let bt_json = serde_json::to_value(job_to_aria2_status(&bt_job)).unwrap();

        assert!(
            range_json.get("bittorrent").is_none() || range_json["bittorrent"].is_null(),
            "range job should not have bittorrent field"
        );
        assert!(
            bt_json["bittorrent"].is_object(),
            "BT job must have bittorrent object"
        );
    }

    /// dir field must be extracted from out_path's parent.
    #[test]
    fn tell_status_dir_is_parent_of_out_path() {
        let job = Job::new_range(vec![], PathBuf::from("/home/user/downloads/file.zip"));
        let status = job_to_aria2_status(&job);
        assert_eq!(status.dir, "/home/user/downloads");
    }

    // ═══════════════════════════════════════════════════════════════════
    // getGlobalStat response parity
    // ═══════════════════════════════════════════════════════════════════

    /// All fields in getGlobalStat must be strings.
    #[test]
    fn global_stat_all_fields_are_strings() {
        let jobs = vec![
            {
                let mut j = Job::new_range(vec![], PathBuf::from("/a"));
                j.status = Status::Active;
                j.download_speed = 50_000;
                j.upload_speed = 10_000;
                j
            },
            {
                let mut j = Job::new_range(vec![], PathBuf::from("/b"));
                j.status = Status::Waiting;
                j
            },
            {
                let mut j = Job::new_range(vec![], PathBuf::from("/c"));
                j.status = Status::Complete;
                j
            },
        ];

        let stat = compute_global_stat(&jobs);
        let json = serde_json::to_value(&stat).unwrap();

        for field in &[
            "downloadSpeed",
            "uploadSpeed",
            "numActive",
            "numWaiting",
            "numStopped",
            "numStoppedTotal",
        ] {
            assert!(
                json[field].is_string(),
                "getGlobalStat field '{}' must be a string, got {:?}",
                field,
                json[field]
            );
        }

        assert_eq!(json["downloadSpeed"], "50000");
        assert_eq!(json["uploadSpeed"], "10000");
        assert_eq!(json["numActive"], "1");
        assert_eq!(json["numWaiting"], "1");
        assert_eq!(json["numStopped"], "1");
    }

    /// Empty job list produces all-zero string values.
    #[test]
    fn global_stat_empty_is_all_zero_strings() {
        let stat = compute_global_stat(&[]);
        assert_eq!(stat.download_speed, "0");
        assert_eq!(stat.upload_speed, "0");
        assert_eq!(stat.num_active, "0");
        assert_eq!(stat.num_waiting, "0");
        assert_eq!(stat.num_stopped, "0");
        assert_eq!(stat.num_stopped_total, "0");
    }

    // ═══════════════════════════════════════════════════════════════════
    // JSON field naming parity (camelCase)
    // ═══════════════════════════════════════════════════════════════════

    /// aria2 uses camelCase for JSON field names, not snake_case.
    /// This is enforced by serde(rename_all = "camelCase").
    #[test]
    fn tell_status_json_uses_camel_case_field_names() {
        let job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        let json = serde_json::to_value(job_to_aria2_status(&job)).unwrap();
        let obj = json.as_object().unwrap();

        // Must have camelCase keys
        assert!(
            obj.contains_key("totalLength"),
            "missing camelCase key 'totalLength'"
        );
        assert!(
            obj.contains_key("completedLength"),
            "missing camelCase key 'completedLength'"
        );
        assert!(
            obj.contains_key("downloadSpeed"),
            "missing camelCase key 'downloadSpeed'"
        );
        assert!(
            obj.contains_key("uploadSpeed"),
            "missing camelCase key 'uploadSpeed'"
        );

        // Must NOT have snake_case keys
        assert!(
            !obj.contains_key("total_length"),
            "snake_case key 'total_length' found"
        );
        assert!(
            !obj.contains_key("completed_length"),
            "snake_case key 'completed_length' found"
        );
        assert!(
            !obj.contains_key("download_speed"),
            "snake_case key 'download_speed' found"
        );
    }

    /// getGlobalStat also uses camelCase.
    #[test]
    fn global_stat_json_uses_camel_case_field_names() {
        let stat = compute_global_stat(&[]);
        let json = serde_json::to_value(&stat).unwrap();
        let obj = json.as_object().unwrap();

        assert!(obj.contains_key("downloadSpeed"));
        assert!(obj.contains_key("uploadSpeed"));
        assert!(obj.contains_key("numActive"));
        assert!(obj.contains_key("numWaiting"));
        assert!(obj.contains_key("numStopped"));
        assert!(obj.contains_key("numStoppedTotal"));
    }
}
