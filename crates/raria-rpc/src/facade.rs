// raria-rpc: RPC Facade — translates internal state to aria2 JSON format.
//
// The facade is the bridge between raria-core's internal data model and
// the aria2-compatible JSON-RPC response format. It does NOT handle
// network I/O — that's the server's job.

use raria_core::job::{Job, JobKind, Status};
use serde::{Deserialize, Serialize};

/// aria2-compatible status response for `tellStatus`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aria2Status {
    /// Download GID (hex string).
    pub gid: String,
    /// Lifecycle status: `active`, `waiting`, `paused`, `error`, `complete`, `removed`.
    pub status: String,
    /// Total file size in bytes (string for aria2 compat).
    pub total_length: String,
    /// Bytes downloaded so far (string).
    pub completed_length: String,
    /// Current download speed in bytes/sec (string).
    pub download_speed: String,
    /// Current upload speed in bytes/sec (string, always "0" for non-BT).
    pub upload_speed: String,
    /// Total uploaded bytes in bytes (string).
    pub upload_length: String,
    /// Number of active connections (string).
    pub connections: String,
    /// Download directory path.
    pub dir: String,
    /// File list associated with this download.
    pub files: Vec<Aria2File>,
    /// aria2 error code (present only on error).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Human-readable error description (present only on error).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// BT info hash (hex) when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_hash: Option<String>,
    /// BT seeder count when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_seeders: Option<String>,
    /// Whether this BT job is currently seeding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seeder: Option<String>,
    /// BT piece count when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_pieces: Option<String>,
    /// BT piece length when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub piece_length: Option<String>,
    /// Jobs automatically followed by this job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followed_by: Option<Vec<String>>,
    /// Predecessor job relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub following: Option<String>,
    /// Parent/group relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub belongs_to: Option<String>,
    /// BitTorrent-specific metadata (present only for BT downloads).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bittorrent: Option<serde_json::Value>,
}

/// aria2-compatible file information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aria2File {
    /// One-based file index (string).
    pub index: String,
    /// Absolute file path.
    pub path: String,
    /// File size in bytes (string).
    pub length: String,
    /// Bytes downloaded for this file (string).
    pub completed_length: String,
    /// Whether this file is selected: `"true"` or `"false"`.
    pub selected: String,
    /// URIs serving this file.
    pub uris: Vec<Aria2Uri>,
}

/// aria2-compatible URI information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aria2Uri {
    /// URI string.
    pub uri: String,
    /// URI status: `"used"` or `"waiting"`.
    pub status: String,
}

/// aria2-compatible global stat response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aria2GlobalStat {
    /// Overall download speed in bytes/sec (string).
    pub download_speed: String,
    /// Overall upload speed in bytes/sec (string).
    pub upload_speed: String,
    /// Number of active downloads (string).
    pub num_active: String,
    /// Number of waiting downloads (string).
    pub num_waiting: String,
    /// Number of stopped downloads in current session (string).
    pub num_stopped: String,
    /// Number of stopped downloads total (string).
    pub num_stopped_total: String,
}

/// Convert an internal Job to an aria2-compatible status response.
pub fn job_to_aria2_status(job: &Job) -> Aria2Status {
    let status_str = match job.status {
        Status::Active | Status::Seeding => "active",
        Status::Waiting => "waiting",
        Status::Paused => "paused",
        Status::Complete => "complete",
        Status::Error => "error",
        Status::Removed => "removed",
    };

    let (error_code, error_message) = if job.status == Status::Error {
        (
            Some("1".into()),
            job.error_msg
                .clone()
                .or_else(|| Some("unknown error".into())),
        )
    } else {
        (None, None)
    };

    let files = if job.kind == JobKind::Bt {
        if let Some(bt_files) = &job.bt_files {
            bt_files
                .iter()
                .map(|file| Aria2File {
                    index: (file.index + 1).to_string(),
                    path: file.path.to_string_lossy().into_owned(),
                    length: file.length.to_string(),
                    completed_length: file.completed_length.to_string(),
                    selected: if file.selected { "true" } else { "false" }.into(),
                    uris: Vec::new(),
                })
                .collect()
        } else {
            vec![Aria2File {
                index: "1".into(),
                path: job.out_path.to_string_lossy().into_owned(),
                length: job.total_size.unwrap_or(0).to_string(),
                completed_length: job.downloaded.to_string(),
                selected: "true".into(),
                uris: job
                    .uris
                    .iter()
                    .map(|u| Aria2Uri {
                        uri: u.clone(),
                        status: "used".into(),
                    })
                    .collect(),
            }]
        }
    } else {
        vec![Aria2File {
            index: "1".into(),
            path: job.out_path.to_string_lossy().into_owned(),
            length: job.total_size.unwrap_or(0).to_string(),
            completed_length: job.downloaded.to_string(),
            selected: "true".into(),
            uris: job
                .uris
                .iter()
                .map(|u| Aria2Uri {
                    uri: u.clone(),
                    status: "used".into(),
                })
                .collect(),
        }]
    };

    let bt = job.bt.as_ref();
    let announce_list = bt.and_then(|bt| bt.announce_list.clone()).unwrap_or_default();
    let bittorrent = if job.kind == JobKind::Bt {
        let mode = if job.bt_files.as_ref().map(|files| files.len() > 1).unwrap_or(false) {
            "multi"
        } else {
            "single"
        };
        let mut obj = serde_json::Map::new();
        obj.insert("mode".into(), serde_json::Value::String(mode.into()));
        obj.insert(
            "announceList".into(),
            serde_json::Value::Array(
                announce_list
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
        if let Some(name) = bt.and_then(|bt| bt.torrent_name.clone()) {
            obj.insert("info".into(), serde_json::json!({ "name": name }));
        }
        Some(serde_json::Value::Object(obj))
    } else {
        None
    };

    Aria2Status {
        gid: format!("{}", job.gid),
        status: status_str.into(),
        total_length: job.total_size.unwrap_or(0).to_string(),
        completed_length: job.downloaded.to_string(),
        download_speed: job.download_speed.to_string(),
        upload_speed: job.upload_speed.to_string(),
        upload_length: bt
            .and_then(|bt| bt.uploaded)
            .unwrap_or(0)
            .to_string(),
        connections: job.connections.to_string(),
        dir: job
            .out_path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        files,
        error_code,
        error_message,
        info_hash: bt.and_then(|bt| bt.info_hash.clone()),
        num_seeders: bt.and_then(|bt| bt.num_seeders.map(|value| value.to_string())),
        seeder: (job.kind == JobKind::Bt).then(|| {
            if job.status == Status::Seeding {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }),
        num_pieces: bt.and_then(|bt| bt.num_pieces.map(|value| value.to_string())),
        piece_length: bt.and_then(|bt| bt.piece_length.map(|value| value.to_string())),
        followed_by: (!job.followed_by.is_empty()).then(|| {
            job.followed_by
                .iter()
                .map(|gid| gid.to_string())
                .collect::<Vec<_>>()
        }),
        following: job.following.map(|gid| gid.to_string()),
        belongs_to: job.belongs_to.map(|gid| gid.to_string()),
        bittorrent,
    }
}

/// Convert a set of jobs into an aria2 global stat.
pub fn compute_global_stat(jobs: &[Job]) -> Aria2GlobalStat {
    let mut dl_speed = 0u64;
    let mut ul_speed = 0u64;
    let mut num_active = 0u32;
    let mut num_waiting = 0u32;
    let mut num_stopped = 0u32;

    for job in jobs {
        match job.status {
            Status::Active | Status::Seeding => {
                num_active += 1;
                dl_speed += job.download_speed;
                ul_speed += job.upload_speed;
            }
            Status::Waiting | Status::Paused => {
                num_waiting += 1;
            }
            Status::Complete | Status::Error | Status::Removed => {
                num_stopped += 1;
            }
        }
    }

    Aria2GlobalStat {
        download_speed: dl_speed.to_string(),
        upload_speed: ul_speed.to_string(),
        num_active: num_active.to_string(),
        num_waiting: num_waiting.to_string(),
        num_stopped: num_stopped.to_string(),
        num_stopped_total: num_stopped.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raria_core::job::{Gid, Job, Status};
    use std::path::PathBuf;

    #[test]
    fn job_to_aria2_status_active() {
        let mut job = Job::new_range(
            vec!["https://example.com/f.zip".into()],
            PathBuf::from("/tmp/downloads/f.zip"),
        );
        job.status = Status::Active;
        job.total_size = Some(10000);
        job.downloaded = 5000;
        job.download_speed = 1024;

        let status = job_to_aria2_status(&job);
        assert_eq!(status.status, "active");
        assert_eq!(status.total_length, "10000");
        assert_eq!(status.completed_length, "5000");
        assert_eq!(status.download_speed, "1024");
        assert!(status.error_code.is_none());
        assert!(status.bittorrent.is_none()); // Range job
    }

    #[test]
    fn job_to_aria2_status_error_includes_code() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.status = Status::Error;
        job.error_msg = Some("connection timeout".into());

        let status = job_to_aria2_status(&job);
        assert_eq!(status.status, "error");
        assert_eq!(status.error_code.as_deref(), Some("1"));
        assert_eq!(status.error_message.as_deref(), Some("connection timeout"));
    }

    #[test]
    fn job_to_aria2_status_bt_has_bittorrent_field() {
        let job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:abc".into()],
            PathBuf::from("/tmp/dl"),
        );
        let status = job_to_aria2_status(&job);
        assert!(status.bittorrent.is_some());
    }

    #[test]
    fn job_to_aria2_status_bt_projects_real_metadata_fields() {
        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:abc".into()],
            PathBuf::from("/tmp/downloads/fixture.iso"),
        );
        job.status = Status::Seeding;
        job.total_size = Some(4096);
        job.downloaded = 2048;
        job.upload_speed = 64;
        job.bt = Some(raria_core::job::BtSnapshot {
            info_hash: Some("0123456789abcdef0123456789abcdef01234567".into()),
            torrent_name: Some("fixture.iso".into()),
            announce_list: Some(vec!["http://tracker.example/announce".into()]),
            uploaded: Some(512),
            num_seeders: Some(7),
            piece_length: Some(1024),
            num_pieces: Some(4),
            ..Default::default()
        });

        let status = job_to_aria2_status(&job);
        assert_eq!(status.upload_length, "512");
        assert_eq!(
            status.info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(status.num_seeders.as_deref(), Some("7"));
        assert_eq!(status.num_pieces.as_deref(), Some("4"));
        assert_eq!(status.piece_length.as_deref(), Some("1024"));
        assert_eq!(status.seeder.as_deref(), Some("true"));

        let bittorrent = status.bittorrent.expect("bt metadata");
        assert_eq!(bittorrent["info"]["name"], "fixture.iso");
        assert_eq!(
            bittorrent["announceList"],
            serde_json::json!(["http://tracker.example/announce"])
        );
    }

    #[test]
    fn job_to_aria2_status_projects_relation_fields() {
        let mut job = Job::new_range(
            vec!["https://example.com/a.bin".into()],
            PathBuf::from("/tmp/a.bin"),
        );
        job.followed_by = vec![Gid::from_raw(2), Gid::from_raw(3)];
        job.following = Some(Gid::from_raw(1));
        job.belongs_to = Some(Gid::from_raw(1));

        let status = job_to_aria2_status(&job);
        assert_eq!(
            status.followed_by,
            Some(vec!["0000000000000002".into(), "0000000000000003".into()])
        );
        assert_eq!(status.following.as_deref(), Some("0000000000000001"));
        assert_eq!(status.belongs_to.as_deref(), Some("0000000000000001"));
    }

    #[test]
    fn job_to_aria2_status_seeding_projects_to_active() {
        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:abc".into()],
            PathBuf::from("/tmp/dl"),
        );
        job.status = Status::Seeding;

        let status = job_to_aria2_status(&job);
        assert_eq!(status.status, "active");
    }

    #[test]
    fn job_to_aria2_status_gid_format() {
        let mut job = Job::new_range(vec![], PathBuf::from("/tmp/f"));
        job.gid = Gid::from_raw(255);
        let status = job_to_aria2_status(&job);
        assert_eq!(status.gid, "00000000000000ff");
    }

    #[test]
    fn job_to_aria2_status_files_and_uris() {
        let job = Job::new_range(
            vec!["https://a.com/f".into(), "https://b.com/f".into()],
            PathBuf::from("/tmp/f.zip"),
        );
        let status = job_to_aria2_status(&job);
        assert_eq!(status.files.len(), 1);
        assert_eq!(status.files[0].uris.len(), 2);
        assert_eq!(status.files[0].uris[0].uri, "https://a.com/f");
        assert_eq!(status.files[0].uris[0].status, "used");
    }

    #[test]
    fn compute_global_stat_mixed() {
        let jobs = vec![
            {
                let mut j = Job::new_range(vec![], PathBuf::from("/a"));
                j.status = Status::Active;
                j.download_speed = 1000;
                j.upload_speed = 100;
                j
            },
            {
                let mut j = Job::new_range(vec![], PathBuf::from("/b"));
                j.status = Status::Active;
                j.download_speed = 2000;
                j
            },
            {
                let mut j = Job::new_range(vec![], PathBuf::from("/c"));
                j.status = Status::Waiting;
                j
            },
            {
                let mut j = Job::new_range(vec![], PathBuf::from("/d"));
                j.status = Status::Complete;
                j
            },
        ];

        let stat = compute_global_stat(&jobs);
        assert_eq!(stat.download_speed, "3000");
        assert_eq!(stat.upload_speed, "100");
        assert_eq!(stat.num_active, "2");
        assert_eq!(stat.num_waiting, "1");
        assert_eq!(stat.num_stopped, "1");
    }

    #[test]
    fn compute_global_stat_counts_seeding_as_active() {
        let jobs = vec![{
            let mut j = Job::new_bt(vec![], PathBuf::from("/seed"));
            j.status = Status::Seeding;
            j.download_speed = 200;
            j.upload_speed = 300;
            j
        }];

        let stat = compute_global_stat(&jobs);
        assert_eq!(stat.download_speed, "200");
        assert_eq!(stat.upload_speed, "300");
        assert_eq!(stat.num_active, "1");
        assert_eq!(stat.num_waiting, "0");
        assert_eq!(stat.num_stopped, "0");
    }

    #[test]
    fn compute_global_stat_empty() {
        let stat = compute_global_stat(&[]);
        assert_eq!(stat.num_active, "0");
        assert_eq!(stat.num_waiting, "0");
        assert_eq!(stat.num_stopped, "0");
    }

    #[test]
    fn aria2_status_serde_roundtrips() {
        let job = Job::new_range(
            vec!["https://example.com/f".into()],
            PathBuf::from("/tmp/f"),
        );
        let status = job_to_aria2_status(&job);
        let json = serde_json::to_string(&status).unwrap();
        let recovered: Aria2Status = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.status, status.status);
    }
}
