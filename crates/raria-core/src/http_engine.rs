use std::{
    io::Cursor,
    net::SocketAddr,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
};

use governor::{Quota, RateLimiter};
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions};
use russh::client;
use russh_sftp::client::SftpSession;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use suppaftp::tokio::AsyncFtpStream;
use tokio::{fs, io::AsyncWriteExt};

use crate::{BittorrentDownloadTask, DownloadTask, Error, RariaConfig, Result, RpcEngine};

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
            self.run_http_task(rpc, task).await?;
        }
        let tasks = rpc.pending_ftp_tasks();
        for task in tasks {
            self.run_ftp_task(rpc, task).await?;
        }
        let tasks = rpc.pending_sftp_tasks();
        for task in tasks {
            self.run_sftp_task(rpc, task).await?;
        }
        let tasks = rpc.pending_bittorrent_tasks();
        for task in tasks {
            self.run_bittorrent_task(rpc, task).await?;
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

    async fn run_http_task(&self, rpc: &mut RpcEngine, task: DownloadTask) -> Result<()> {
        let path = self.output_path(&task.uri, task.out.as_deref());
        let control_path = self.control_path(&path);
        let completed = read_control_file(&control_path).await?;
        let bytes = if completed == 0 && task.split.unwrap_or(1) > 1 {
            self.download_split(&task).await?
        } else {
            let range = (completed > 0).then(|| format!("bytes={completed}-"));
            self.download_range(&task, range.as_deref()).await?
        };
        self.finish_task(rpc, &task, &path, &control_path, completed, &bytes)
            .await
    }

    async fn run_ftp_task(&self, rpc: &mut RpcEngine, task: DownloadTask) -> Result<()> {
        let path = self.output_path(&task.uri, task.out.as_deref());
        let control_path = self.control_path(&path);
        let completed = read_control_file(&control_path).await?;
        let bytes = self.download_ftp(&task, completed).await?;
        self.finish_task(rpc, &task, &path, &control_path, completed, &bytes)
            .await
    }

    async fn run_sftp_task(&self, rpc: &mut RpcEngine, task: DownloadTask) -> Result<()> {
        let path = self.output_path(&task.uri, task.out.as_deref());
        let control_path = self.control_path(&path);
        let completed = read_control_file(&control_path).await?;
        let bytes = self.download_sftp(&task, completed).await?;
        self.finish_task(rpc, &task, &path, &control_path, completed, &bytes)
            .await
    }

    async fn run_bittorrent_task(
        &self,
        rpc: &mut RpcEngine,
        task: BittorrentDownloadTask,
    ) -> Result<()> {
        let session = Session::new_with_opts(
            self.config.download_dir.clone(),
            SessionOptions {
                disable_dht: task.initial_peers.is_empty(),
                disable_dht_persistence: true,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| Error::Download(error.to_string()))?;
        let initial_peers = task
            .initial_peers
            .iter()
            .map(|peer| {
                peer.parse::<SocketAddr>().map_err(|error| {
                    Error::Download(format!("invalid bt-initial-peer '{peer}': {error}"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let only_files = task
            .selected_files
            .as_ref()
            .map(|files| files.iter().map(|file| file - 1).collect());
        let add = match (&task.torrent_bytes, &task.magnet_uri) {
            (Some(bytes), _) => AddTorrent::TorrentFileBytes(bytes.clone().into()),
            (None, Some(uri)) => AddTorrent::from_url(uri.as_str()),
            (None, None) => {
                return Err(Error::Download("BitTorrent task has no metadata".into()));
            }
        };
        let response = session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    initial_peers: Some(initial_peers),
                    only_files,
                    overwrite: true,
                    output_folder: Some(self.config.download_dir.to_string_lossy().into_owned()),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let handle = match response {
            AddTorrentResponse::Added(_, handle)
            | AddTorrentResponse::AlreadyManaged(_, handle) => handle,
            AddTorrentResponse::ListOnly(_) => {
                return Err(Error::Download("BitTorrent task returned list-only".into()));
            }
        };
        handle
            .wait_until_completed()
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let completed_length = handle.stats().progress_bytes;
        session.stop().await;
        rpc.complete_task(&task.gid, completed_length)
            .map_err(|error| Error::Download(error.message))?;
        Ok(())
    }

    async fn finish_task(
        &self,
        rpc: &mut RpcEngine,
        task: &DownloadTask,
        path: &Path,
        control_path: &Path,
        completed: u64,
        bytes: &[u8],
    ) -> Result<()> {
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
                .open(path)
                .await
                .map_err(|error| Error::Download(error.to_string()))?;
            file.write_all(bytes)
                .await
                .map_err(|error| Error::Download(error.to_string()))?;
        } else {
            fs::write(path, bytes)
                .await
                .map_err(|error| Error::Download(error.to_string()))?;
        }
        let completed_length = completed + bytes.len() as u64;
        if let Some(message) = checksum_error(task.checksum.as_deref(), path).await? {
            let _ = fs::remove_file(control_path).await;
            rpc.fail_task(&task.gid, message)
                .map_err(|error| Error::Download(error.message))?;
            return Ok(());
        }
        let _ = fs::remove_file(control_path).await;
        rpc.complete_task(&task.gid, completed_length)
            .map_err(|error| Error::Download(error.message))?;
        Ok(())
    }

    async fn download_split(&self, task: &DownloadTask) -> Result<bytes::Bytes> {
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

    async fn probe_length(&self, task: &DownloadTask) -> Result<u64> {
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

    async fn download_range(
        &self,
        task: &DownloadTask,
        range: Option<&str>,
    ) -> Result<bytes::Bytes> {
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
        task: &DownloadTask,
        range: Option<&str>,
    ) -> Result<reqwest::RequestBuilder> {
        let client = if let Some(proxy) = &task.http_proxy {
            reqwest::Client::builder()
                .proxy(reqwest::Proxy::http(proxy).map_err(|error| {
                    Error::Download(format!("invalid http-proxy option: {error}"))
                })?)
                .build()
                .map_err(|error| Error::Download(error.to_string()))?
        } else {
            self.client.clone()
        };
        let mut request = client.get(&task.uri);
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
        if let Some(path) = &task.netrc_path
            && let Some((login, password)) = netrc_credentials(path, &task.uri).await?
        {
            request = request.basic_auth(login, Some(password));
        }
        if let Some(range) = range {
            request = request.header(reqwest::header::RANGE, range);
        }
        Ok(request)
    }

    async fn download_ftp(&self, task: &DownloadTask, completed: u64) -> Result<Vec<u8>> {
        let url =
            reqwest::Url::parse(&task.uri).map_err(|error| Error::Download(error.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| Error::Download("FTP URI is missing a host".into()))?;
        let port = url.port().unwrap_or(21);
        let username = task.ftp_user.as_deref().unwrap_or_else(|| {
            if url.username().is_empty() {
                "anonymous"
            } else {
                url.username()
            }
        });
        let password = task
            .ftp_passwd
            .as_deref()
            .or_else(|| url.password())
            .unwrap_or("anonymous@");
        let path = url.path().trim_start_matches('/');
        let mut ftp = AsyncFtpStream::connect((host, port))
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        ftp.login(username, password)
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        if completed > 0 {
            ftp.resume_transfer(completed as usize)
                .await
                .map_err(|error| Error::Download(error.to_string()))?;
        }
        let bytes = ftp
            .retr(path, |mut stream| {
                Box::pin(async move {
                    let mut bytes = Vec::new();
                    tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut bytes)
                        .await
                        .map_err(suppaftp::FtpError::ConnectionError)?;
                    Ok((bytes, stream))
                })
            })
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let _ = ftp.quit().await;
        Ok(bytes)
    }

    async fn download_sftp(&self, task: &DownloadTask, completed: u64) -> Result<Vec<u8>> {
        let url =
            reqwest::Url::parse(&task.uri).map_err(|error| Error::Download(error.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| Error::Download("SFTP URI is missing a host".into()))?;
        let port = url.port().unwrap_or(22);
        let username = task.ftp_user.as_deref().unwrap_or_else(|| {
            if url.username().is_empty() {
                "anonymous"
            } else {
                url.username()
            }
        });
        let password = task
            .ftp_passwd
            .as_deref()
            .or_else(|| url.password())
            .unwrap_or("");
        let path = url.path();
        let config = client::Config::default();
        let mut session = client::connect(Arc::new(config), (host, port), AcceptAnyServerKey)
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let auth = session
            .authenticate_password(username, password)
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        if !auth.success() {
            return Err(Error::Download("SFTP authentication failed".into()));
        }
        let channel = session
            .channel_open_session()
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let mut file = sftp
            .open(path)
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        if completed > 0 {
            tokio::io::AsyncSeekExt::seek(&mut file, std::io::SeekFrom::Start(completed))
                .await
                .map_err(|error| Error::Download(error.to_string()))?;
        }
        let mut bytes = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut file, &mut bytes)
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
        let _ = file.shutdown().await;
        let _ = sftp.close().await;
        Ok(bytes)
    }
}

#[derive(Debug)]
struct SftpClientError(String);

impl std::fmt::Display for SftpClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SftpClientError {}

impl From<russh::Error> for SftpClientError {
    fn from(error: russh::Error) -> Self {
        Self(error.to_string())
    }
}

struct AcceptAnyServerKey;

impl client::Handler for AcceptAnyServerKey {
    type Error = SftpClientError;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn netrc_credentials(path: &str, uri: &str) -> Result<Option<(String, String)>> {
    let text = fs::read_to_string(path)
        .await
        .map_err(|error| Error::Download(error.to_string()))?;
    let netrc = netrc::Netrc::parse(Cursor::new(text))
        .map_err(|error| Error::Download(format!("{error:?}")))?;
    let url = reqwest::Url::parse(uri).map_err(|error| Error::Download(error.to_string()))?;
    let Some(host) = url.host_str() else {
        return Ok(None);
    };
    let port = url.port();
    let machine = netrc
        .hosts
        .iter()
        .find(|(name, machine)| name == host && (machine.port.is_none() || machine.port == port))
        .map(|(_, machine)| machine)
        .or(netrc.default.as_ref());
    Ok(machine.and_then(|machine| {
        machine
            .password
            .as_ref()
            .map(|password| (machine.login.clone(), password.clone()))
    }))
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
