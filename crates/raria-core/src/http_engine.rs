use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio::{fs, io::AsyncWriteExt};

use crate::{Error, RariaConfig, Result, RpcEngine};

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
            let mut request = self.client.get(&task.uri);
            if completed > 0 {
                request = request.header(reqwest::header::RANGE, format!("bytes={completed}-"));
            }
            let response = request
                .send()
                .await
                .map_err(|error| Error::Download(error.to_string()))?
                .error_for_status()
                .map_err(|error| Error::Download(error.to_string()))?;
            let bytes = response
                .bytes()
                .await
                .map_err(|error| Error::Download(error.to_string()))?;
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
