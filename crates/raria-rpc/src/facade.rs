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
    pub gid: String,
    pub status: String,
    pub total_length: String,
    pub completed_length: String,
    pub download_speed: String,
    pub upload_speed: String,
    pub connections: String,
    pub dir: String,
    pub files: Vec<Aria2File>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bittorrent: Option<serde_json::Value>,
}

/// aria2-compatible file information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aria2File {
    pub index: String,
    pub path: String,
    pub length: String,
    pub completed_length: String,
    pub selected: String,
    pub uris: Vec<Aria2Uri>,
}

/// aria2-compatible URI information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aria2Uri {
    pub uri: String,
    pub status: String,
}

/// aria2-compatible global stat response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aria2GlobalStat {
    pub download_speed: String,
    pub upload_speed: String,
    pub num_active: String,
    pub num_waiting: String,
    pub num_stopped: String,
    pub num_stopped_total: String,
}

/// Convert an internal Job to an aria2-compatible status response.
pub fn job_to_aria2_status(job: &Job) -> Aria2Status {
    let status_str = match job.status {
        Status::Active => "active",
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

    Aria2Status {
        gid: format!("{}", job.gid),
        status: status_str.into(),
        total_length: job.total_size.unwrap_or(0).to_string(),
        completed_length: job.downloaded.to_string(),
        download_speed: job.download_speed.to_string(),
        upload_speed: job.upload_speed.to_string(),
        connections: job.connections.to_string(),
        dir: job
            .out_path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        files,
        error_code,
        error_message,
        bittorrent: if job.kind == JobKind::Bt {
            Some(serde_json::json!({}))
        } else {
            None
        },
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
            Status::Active => {
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
