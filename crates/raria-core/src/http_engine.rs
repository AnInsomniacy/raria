use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
};

use governor::{Quota, RateLimiter};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt};

use crate::{Error, HttpTask, RariaConfig, Result, RpcEngine};

pub struct DownloadEngine {
    config: RariaConfig,
    client: reqwest::Client,
}

impl DownloadEngine {
    pub fn new(config: RariaConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub async fn run_once(&self, rpc: &mut RpcEngine) -> Result<()> {
        let tasks = rpc.pending_http_tasks();
        for task in tasks {
            let path = self.output_path(&task.uri, task.out.as_deref());
            let control_path = self.control_path(&path);
            let completed = read_control_file(&control_path).await?;
            let bytes = if completed == 0 && task.split.unwrap_or(1) > 1 {
                self.download_split(&task).await?
            } else {
                let range = (completed > 0).then(|| format!("bytes={completed}-"));
                self.download_range(&task, range.as_deref()).await?
            };
            if let Some(limit) = task.max_download_limit {
                throttle_bytes(limit, bytes.len()).await?;
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|error| Error::Download(error.to_string()))?;
            }
            if completed > 0 {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .await
                    .map_err(|error| Error::Download(error.to_string()))?;
                file.write_all(&bytes)
                    .await
                    .map_err(|error| Error::Download(error.to_string()))?;
            } else {
                fs::write(&path, &bytes)
                    .await
                    .map_err(|error| Error::Download(error.to_string()))?;
            }
            let completed_length = completed + bytes.len() as u64;
            if let Some(message) = checksum_error(task.checksum.as_deref(), &path).await? {
                let _ = fs::remove_file(&control_path).await;
                rpc.fail_task(&task.gid, message)
                    .map_err(|error| Error::Download(error.message))?;
                continue;
            }
            let _ = fs::remove_file(&control_path).await;
            rpc.complete_task(&task.gid, completed_length)
                .map_err(|error| Error::Download(error.message))?;
        }
        Ok(())
    }

    fn output_path(&self, uri: &str, out: Option<&str>) -> PathBuf {
        let name = out
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| uri.rsplit('/').next().unwrap_or("download").to_owned());
        self.config.download_dir.join(Path::new(&name))
    }

    fn control_path(&self, output_path: &Path) -> PathBuf {
        PathBuf::from(format!(
            "{}{}",
            output_path.display(),
            self.config.control_file_extension
        ))
    }

    async fn download_split(&self, task: &HttpTask) -> Result<bytes::Bytes> {
        let total_length = self.probe_length(task).await?;
        let split = u64::from(task.split.unwrap_or(1).max(1));
        let chunk_size = total_length.div_ceil(split);
        let mut out = Vec::new();
        for index in 0..split {
            let start = index * chunk_size;
            if start >= total_length {
                break;
            }
            let end = ((start + chunk_size).min(total_length)) - 1;
            let range = if index + 1 == split {
                format!("bytes={start}-")
            } else {
                format!("bytes={start}-{end}")
            };
            out.extend_from_slice(&self.download_range(task, Some(&range)).await?);
        }
        Ok(out.into())
    }

    async fn probe_length(&self, task: &HttpTask) -> Result<u64> {
        let response = self
            .prepare_request(task, Some("bytes=0-0"))
            .await?
            .send()
            .await
            .map_err(|error| Error::Download(error.to_string()))?
            .error_for_status()
            .map_err(|error| Error::Download(error.to_string()))?;
        let header = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| Error::Download("missing content-range for split download".into()))?;
        let (_, total) = header
            .rsplit_once('/')
            .ok_or_else(|| Error::Download(format!("invalid content-range: {header}")))?;
        total
            .parse::<u64>()
            .map_err(|error| Error::Download(error.to_string()))
    }

    async fn download_range(&self, task: &HttpTask, range: Option<&str>) -> Result<bytes::Bytes> {
        self.prepare_request(task, range)
            .await?
            .send()
            .await
            .map_err(|error| Error::Download(error.to_string()))?
            .error_for_status()
            .map_err(|error| Error::Download(error.to_string()))?
            .bytes()
            .await
            .map_err(|error| Error::Download(error.to_string()))
    }

    async fn prepare_request(
        &self,
        task: &HttpTask,
        range: Option<&str>,
    ) -> Result<reqwest::RequestBuilder> {
        let mut request = self.client.get(&task.uri);
        if let Some(header) = &task.header {
            let (name, value) = header
                .split_once(':')
                .ok_or_else(|| Error::Download(format!("invalid header option: {header}")))?;
            request = request.header(name.trim(), value.trim());
        }
        if let Some(path) = &task.load_cookies {
            let cookie = fs::read_to_string(path)
                .await
                .map_err(|error| Error::Download(error.to_string()))?;
            request = request.header(reqwest::header::COOKIE, cookie.trim());
        }
        if let Some(range) = range {
            request = request.header(reqwest::header::RANGE, range);
        }
        Ok(request)
    }
}

async fn throttle_bytes(bytes_per_second: u32, byte_count: usize) -> Result<()> {
    if byte_count == 0 {
        return Ok(());
    }
    let quota = NonZeroU32::new(bytes_per_second)
        .map(Quota::per_second)
        .map(|quota| quota.allow_burst(NonZeroU32::new(1).expect("one is non-zero")))
        .ok_or_else(|| Error::Download("max-download-limit must be greater than zero".into()))?;
    let limiter = RateLimiter::direct(quota);
    for _ in 0..byte_count {
        limiter.until_ready().await;
    }
    Ok(())
}

async fn checksum_error(checksum: Option<&str>, path: &Path) -> Result<Option<String>> {
    let Some(checksum) = checksum else {
        return Ok(None);
    };
    let Some(expected) = checksum.strip_prefix("sha-256=") else {
        return Ok(Some(format!("unsupported checksum format: {checksum}")));
    };
    let bytes = fs::read(path)
        .await
        .map_err(|error| Error::Download(error.to_string()))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual == expected {
        Ok(None)
    } else {
        Ok(Some(format!(
            "checksum mismatch: expected sha-256={expected}, actual sha-256={actual}"
        )))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlFile {
    completed_length: u64,
}

async fn read_control_file(path: &Path) -> Result<u64> {
    match fs::read_to_string(path).await {
        Ok(text) => {
            let control: ControlFile =
                serde_json::from_str(&text).map_err(|error| Error::Download(error.to_string()))?;
            Ok(control.completed_length)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(Error::Download(error.to_string())),
    }
}
