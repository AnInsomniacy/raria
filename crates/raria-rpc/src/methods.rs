// raria-rpc: JSON-RPC method handlers — full aria2 RPC parity.
//
// Implements all 27 aria2-compatible JSON-RPC methods using jsonrpsee.
//
// Complete method list:
// ─ Download control ─
//   aria2.addUri, aria2.addTorrent, aria2.addMetalink
//   aria2.remove, aria2.forceRemove
//   aria2.pause, aria2.pauseAll, aria2.forcePause, aria2.forcePauseAll
//   aria2.unpause, aria2.unpauseAll
// ─ Query ─
//   aria2.tellStatus, aria2.getUris, aria2.getFiles, aria2.getPeers, aria2.getServers
//   aria2.tellActive, aria2.tellWaiting, aria2.tellStopped
//   aria2.getGlobalStat, aria2.getVersion, aria2.getSessionInfo
// ─ Configuration ─
//   aria2.changeOption, aria2.getOption
//   aria2.changeGlobalOption, aria2.getGlobalOption
//   aria2.changePosition
// ─ Session ─
//   aria2.purgeDownloadResult, aria2.removeDownloadResult
//   aria2.saveSession, aria2.shutdown, aria2.forceShutdown

use crate::facade;
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use raria_core::engine::{AddUriSpec, Engine, PositionHow};
use raria_core::job::Status;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// aria2-style request options (per-download overrides).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RpcOptions {
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default, rename = "out")]
    pub filename: Option<String>,
    #[serde(default, rename = "split")]
    pub connections: Option<String>,
    #[serde(default, rename = "max-download-limit")]
    pub max_download_limit: Option<String>,
    #[serde(default, rename = "header")]
    pub header: Option<Vec<String>>,
    #[serde(default, rename = "checksum")]
    pub checksum: Option<String>,
    #[serde(default, rename = "http-user")]
    pub http_user: Option<String>,
    #[serde(default, rename = "http-passwd")]
    pub http_passwd: Option<String>,
    #[serde(default, rename = "select-file")]
    pub select_file: Option<String>,
}

/// JSON-RPC interface definition — full aria2 parity.
#[rpc(server)]
pub trait Aria2Rpc {
    // ── Download control ─────────────────────────────────────────────

    #[method(name = "aria2.addUri")]
    async fn add_uri(
        &self,
        uris: Vec<String>,
        options: Option<RpcOptions>,
    ) -> RpcResult<String>;

    #[method(name = "aria2.addTorrent")]
    async fn add_torrent(
        &self,
        torrent_base64: String,
        uris: Option<Vec<String>>,
        options: Option<RpcOptions>,
    ) -> RpcResult<String>;

    #[method(name = "aria2.addMetalink")]
    async fn add_metalink(
        &self,
        metalink_base64: String,
        options: Option<RpcOptions>,
    ) -> RpcResult<Vec<String>>;

    #[method(name = "aria2.remove")]
    async fn remove(&self, gid: String) -> RpcResult<String>;

    #[method(name = "aria2.forceRemove")]
    async fn force_remove(&self, gid: String) -> RpcResult<String>;

    #[method(name = "aria2.pause")]
    async fn pause(&self, gid: String) -> RpcResult<String>;

    #[method(name = "aria2.pauseAll")]
    async fn pause_all(&self) -> RpcResult<String>;

    #[method(name = "aria2.forcePause")]
    async fn force_pause(&self, gid: String) -> RpcResult<String>;

    #[method(name = "aria2.forcePauseAll")]
    async fn force_pause_all(&self) -> RpcResult<String>;

    #[method(name = "aria2.unpause")]
    async fn unpause(&self, gid: String) -> RpcResult<String>;

    #[method(name = "aria2.unpauseAll")]
    async fn unpause_all(&self) -> RpcResult<String>;

    // ── Query ────────────────────────────────────────────────────────

    #[method(name = "aria2.tellStatus")]
    async fn tell_status(&self, gid: String) -> RpcResult<serde_json::Value>;

    #[method(name = "aria2.getUris")]
    async fn get_uris(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    #[method(name = "aria2.getFiles")]
    async fn get_files(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    #[method(name = "aria2.getPeers")]
    async fn get_peers(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    #[method(name = "aria2.getServers")]
    async fn get_servers(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    #[method(name = "aria2.tellActive")]
    async fn tell_active(&self) -> RpcResult<Vec<serde_json::Value>>;

    #[method(name = "aria2.tellWaiting")]
    async fn tell_waiting(&self, offset: i64, num: u32) -> RpcResult<Vec<serde_json::Value>>;

    #[method(name = "aria2.tellStopped")]
    async fn tell_stopped(&self, offset: i64, num: u32) -> RpcResult<Vec<serde_json::Value>>;

    #[method(name = "aria2.getGlobalStat")]
    async fn get_global_stat(&self) -> RpcResult<serde_json::Value>;

    #[method(name = "aria2.getVersion")]
    async fn get_version(&self) -> RpcResult<serde_json::Value>;

    #[method(name = "aria2.getSessionInfo")]
    async fn get_session_info(&self) -> RpcResult<serde_json::Value>;

    // ── Configuration ────────────────────────────────────────────────

    #[method(name = "aria2.changeOption")]
    async fn change_option(
        &self,
        gid: String,
        options: serde_json::Value,
    ) -> RpcResult<String>;

    #[method(name = "aria2.getOption")]
    async fn get_option(&self, gid: String) -> RpcResult<serde_json::Value>;

    #[method(name = "aria2.changeGlobalOption")]
    async fn change_global_option(&self, options: serde_json::Value) -> RpcResult<String>;

    #[method(name = "aria2.getGlobalOption")]
    async fn get_global_option(&self) -> RpcResult<serde_json::Value>;

    #[method(name = "aria2.changePosition")]
    async fn change_position(
        &self,
        gid: String,
        pos: i32,
        how: String,
    ) -> RpcResult<i64>;

    // ── Session management ───────────────────────────────────────────

    #[method(name = "aria2.purgeDownloadResult")]
    async fn purge_download_result(&self) -> RpcResult<String>;

    #[method(name = "aria2.removeDownloadResult")]
    async fn remove_download_result(&self, gid: String) -> RpcResult<String>;

    #[method(name = "aria2.saveSession")]
    async fn save_session(&self) -> RpcResult<String>;

    #[method(name = "aria2.shutdown")]
    async fn shutdown(&self) -> RpcResult<String>;

    #[method(name = "aria2.forceShutdown")]
    async fn force_shutdown(&self) -> RpcResult<String>;
}

/// RPC server state that holds a reference to the Engine.
pub struct RpcHandler {
    engine: Arc<Engine>,
}

impl RpcHandler {
    /// Create a new RPC handler wrapping the given engine.
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }
}

#[async_trait::async_trait]
impl Aria2RpcServer for RpcHandler {
    // ── Download control ─────────────────────────────────────────────

    async fn add_uri(
        &self,
        uris: Vec<String>,
        options: Option<RpcOptions>,
    ) -> RpcResult<String> {
        let opts = options.unwrap_or_default();
        let dir = opts
            .dir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.engine.config.dir.clone());
        let connections = opts
            .connections
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(16);

        let spec = AddUriSpec {
            uris,
            dir,
            filename: opts.filename,
            connections,
        };

        let handle = self
            .engine
            .add_uri(&spec)
            .map_err(|e| rpc_err(1, &e.to_string()))?;

        // Apply per-job options from RPC request.
        let gid = handle.gid;
        self.engine.registry.update(gid, |job| {
            if let Some(ref conns) = opts.connections {
                if let Ok(n) = conns.parse::<u32>() {
                    job.options.max_connections = n;
                }
            }
            if let Some(ref limit) = opts.max_download_limit {
                if let Ok(bps) = limit.parse::<u64>() {
                    job.options.max_download_limit = bps;
                }
            }
            if let Some(ref headers) = opts.header {
                for h in headers {
                    if let Some((key, value)) = h.split_once(':') {
                        job.options.headers.push((
                            key.trim().to_string(),
                            value.trim().to_string(),
                        ));
                    }
                }
            }
            if let Some(ref cksum) = opts.checksum {
                job.options.checksum = Some(cksum.clone());
            }
            if let Some(ref user) = opts.http_user {
                job.options.http_user = Some(user.clone());
            }
            if let Some(ref passwd) = opts.http_passwd {
                job.options.http_passwd = Some(passwd.clone());
            }
            if let Some(ref select_file) = opts.select_file {
                if let Ok(files) = parse_select_file_spec(select_file) {
                    job.options.bt_selected_files = Some(files);
                }
            }
        });

        debug!(gid = %handle.gid, "RPC addUri succeeded");
        Ok(format!("{}", handle.gid))
    }

    async fn add_torrent(
        &self,
        torrent_base64: String,
        _uris: Option<Vec<String>>,
        options: Option<RpcOptions>,
    ) -> RpcResult<String> {
        use base64::Engine as Base64Engine;
        use raria_core::job::{Gid, Job};

        // Decode base64 → torrent bytes.
        let torrent_bytes = base64::engine::general_purpose::STANDARD
            .decode(&torrent_base64)
            .map_err(|e| rpc_err(1, &format!("invalid base64: {e}")))?;

        if torrent_bytes.is_empty() {
            return Err(rpc_err(1, "empty torrent data"));
        }

        // Store the raw torrent bytes as a base64 data URI so the daemon
        // can retrieve them when it activates this job.
        let torrent_uri = format!("torrent:base64:{torrent_base64}");

        let _gid = Gid::new();
        let out_path = self.engine.config.dir.join("bt_download");
        let mut job = Job::new_bt(vec![torrent_uri], out_path);
        if let Some(select_file) = options
            .and_then(|opts| opts.select_file)
            .map(|spec| parse_select_file_spec(&spec))
            .transpose()
            .map_err(|e| rpc_err(1, &e.to_string()))?
        {
            job.options.bt_selected_files = Some(select_file);
        }
        let actual_gid = job.gid;

        self.engine.cancel_registry.register(actual_gid);
        self.engine
            .registry
            .insert(job)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        self.engine.scheduler.enqueue(actual_gid);
        self.engine.work_notify().notify_one();

        let gid_str = format!("{:016x}", actual_gid.as_raw());
        debug!(gid = %gid_str, "addTorrent: BT job created");
        Ok(gid_str)
    }

    async fn add_metalink(
        &self,
        metalink_base64: String,
        _options: Option<RpcOptions>,
    ) -> RpcResult<Vec<String>> {
        use base64::Engine as Base64Engine;
        use raria_core::engine::AddUriSpec;
        use raria_metalink::parser::parse_metalink;

        // Decode base64 → XML bytes.
        let xml_bytes = base64::engine::general_purpose::STANDARD
            .decode(&metalink_base64)
            .map_err(|e| rpc_err(1, &format!("invalid base64: {e}")))?;

        let xml_str = String::from_utf8(xml_bytes)
            .map_err(|e| rpc_err(1, &format!("metalink is not valid UTF-8: {e}")))?;

        // Parse the Metalink XML.
        let metalink = parse_metalink(&xml_str)
            .map_err(|e| rpc_err(1, &format!("failed to parse metalink: {e}")))?;

        if metalink.files.is_empty() {
            return Err(rpc_err(1, "metalink contains no files"));
        }

        // Create a job for each file in the metalink.
        let mut gids = Vec::new();
        for file in &metalink.files {
            let uris: Vec<String> = file.urls.iter().map(|u| u.url.clone()).collect();
            if uris.is_empty() {
                continue;
            }

            let spec = AddUriSpec {
                uris: uris.clone(),
                filename: Some(file.name.clone()),
                dir: self.engine.config.dir.clone(),
                connections: 16,
            };

            match self.engine.add_uri(&spec) {
                Ok(handle) => {
                    let gid_str = format!("{:016x}", handle.gid.as_raw());
                    debug!(gid = %gid_str, name = %file.name, "metalink: added job");
                    gids.push(gid_str);
                }
                Err(e) => {
                    warn!(name = %file.name, error = %e, "metalink: failed to add job");
                }
            }
        }

        if gids.is_empty() {
            return Err(rpc_err(1, "no downloadable files found in metalink"));
        }

        Ok(gids)
    }

    async fn remove(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine.remove(parsed_gid).map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn force_remove(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine.force_remove(parsed_gid).map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn pause(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine.pause(parsed_gid).map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn pause_all(&self) -> RpcResult<String> {
        self.engine.pause_all();
        Ok("OK".into())
    }

    async fn force_pause(&self, gid: String) -> RpcResult<String> {
        // In aria2, forcePause is like pause but doesn't wait for piece completion.
        // For raria, pause is already immediate since we cancel tokens.
        let parsed_gid = parse_gid(&gid)?;
        self.engine.pause(parsed_gid).map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn force_pause_all(&self) -> RpcResult<String> {
        self.engine.pause_all();
        Ok("OK".into())
    }

    async fn unpause(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine.unpause(parsed_gid).map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn unpause_all(&self) -> RpcResult<String> {
        self.engine.unpause_all();
        Ok("OK".into())
    }

    // ── Query ────────────────────────────────────────────────────────

    async fn tell_status(&self, gid: String) -> RpcResult<serde_json::Value> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self.engine.registry.get(parsed_gid).ok_or_else(|| gid_not_found(&gid))?;
        let status = facade::job_to_aria2_status(&job);
        serde_json::to_value(&status).map_err(|e| internal_error(&e.to_string()))
    }

    async fn get_uris(&self, gid: String) -> RpcResult<Vec<serde_json::Value>> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self.engine.registry.get(parsed_gid).ok_or_else(|| gid_not_found(&gid))?;
        let uris: Vec<serde_json::Value> = job
            .uris
            .iter()
            .map(|u| {
                serde_json::json!({
                    "uri": u,
                    "status": "used"
                })
            })
            .collect();
        Ok(uris)
    }

    async fn get_files(&self, gid: String) -> RpcResult<Vec<serde_json::Value>> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self.engine.registry.get(parsed_gid).ok_or_else(|| gid_not_found(&gid))?;
        let status = facade::job_to_aria2_status(&job);
        let files: Vec<serde_json::Value> = status
            .files
            .iter()
            .map(|f| serde_json::to_value(f).unwrap_or_default())
            .collect();
        Ok(files)
    }

    async fn get_peers(&self, _gid: String) -> RpcResult<Vec<serde_json::Value>> {
        // BT peers — not applicable for HTTP/FTP/SFTP downloads.
        Ok(vec![])
    }

    async fn get_servers(&self, gid: String) -> RpcResult<Vec<serde_json::Value>> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self.engine.registry.get(parsed_gid).ok_or_else(|| gid_not_found(&gid))?;
        let servers: Vec<serde_json::Value> = job
            .uris
            .iter()
            .enumerate()
            .map(|(i, u)| {
                serde_json::json!({
                    "index": (i + 1).to_string(),
                    "servers": [{
                        "uri": u,
                        "currentUri": u,
                        "downloadSpeed": job.download_speed.to_string()
                    }]
                })
            })
            .collect();
        Ok(servers)
    }

    async fn tell_active(&self) -> RpcResult<Vec<serde_json::Value>> {
        let jobs = self.engine.registry.by_status(Status::Active);
        jobs_to_json(&jobs)
    }

    async fn tell_waiting(&self, offset: i64, num: u32) -> RpcResult<Vec<serde_json::Value>> {
        let mut jobs = self.engine.registry.by_status(Status::Waiting);
        jobs.extend(self.engine.registry.by_status(Status::Paused));
        apply_offset_limit(&mut jobs, offset, num);
        jobs_to_json(&jobs)
    }

    async fn tell_stopped(&self, offset: i64, num: u32) -> RpcResult<Vec<serde_json::Value>> {
        let mut jobs = self.engine.registry.by_status(Status::Complete);
        jobs.extend(self.engine.registry.by_status(Status::Error));
        jobs.extend(self.engine.registry.by_status(Status::Removed));
        apply_offset_limit(&mut jobs, offset, num);
        jobs_to_json(&jobs)
    }

    async fn get_global_stat(&self) -> RpcResult<serde_json::Value> {
        let jobs = self.engine.registry.snapshot();
        let stat = facade::compute_global_stat(&jobs);
        serde_json::to_value(&stat).map_err(|e| internal_error(&e.to_string()))
    }

    async fn get_version(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "enabledFeatures": [
                "HTTP",
                "HTTPS",
                "FTP",
                "SFTP",
                "BitTorrent",
                "Metalink"
            ]
        }))
    }

    async fn get_session_info(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!({
            "sessionId": self.engine.session_id
        }))
    }

    // ── Configuration ────────────────────────────────────────────────

    async fn change_option(
        &self,
        gid: String,
        options: serde_json::Value,
    ) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        let _job = self.engine.registry.get(parsed_gid).ok_or_else(|| gid_not_found(&gid))?;

        // Apply supported per-job options.
        self.engine.registry.update(parsed_gid, |job| {
            if let Some(limit) = options.get("max-download-limit").and_then(|v| v.as_str()) {
                if let Ok(bps) = limit.parse::<u64>() {
                    job.options.max_download_limit = bps;
                    debug!(%gid, bps, "changed max-download-limit");
                }
            }
            if let Some(limit) = options.get("max-upload-limit").and_then(|v| v.as_str()) {
                if let Ok(bps) = limit.parse::<u64>() {
                    job.options.max_upload_limit = bps;
                    debug!(%gid, bps, "changed max-upload-limit");
                }
            }
            if let Some(conns) = options.get("max-connection-per-server").and_then(|v| v.as_str()) {
                if let Ok(n) = conns.parse::<u32>() {
                    job.options.max_connections = n;
                    debug!(%gid, n, "changed max-connection-per-server");
                }
            }
            if let Some(conns) = options.get("split").and_then(|v| v.as_str()) {
                if let Ok(n) = conns.parse::<u32>() {
                    job.options.max_connections = n;
                    debug!(%gid, n, "changed split");
                }
            }
        });
        Ok("OK".into())
    }

    async fn get_option(&self, gid: String) -> RpcResult<serde_json::Value> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self.engine.registry.get(parsed_gid).ok_or_else(|| gid_not_found(&gid))?;

        Ok(serde_json::json!({
            "dir": job.out_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            "out": job.out_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "max-download-limit": job.options.max_download_limit.to_string(),
            "max-upload-limit": job.options.max_upload_limit.to_string(),
            "split": job.options.max_connections.to_string(),
            "min-split-size": "1048576",
            "max-connection-per-server": job.options.max_connections.to_string(),
            "header": job.options.headers.iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>(),
            "checksum": job.options.checksum.as_deref().unwrap_or(""),
            "http-user": job.options.http_user.as_deref().unwrap_or(""),
            "http-passwd": job.options.http_passwd.as_deref().unwrap_or(""),
            "select-file": job.options.bt_selected_files.as_ref()
                .map(|files| files.iter().map(|idx| (idx + 1).to_string()).collect::<Vec<_>>().join(","))
                .unwrap_or_default(),
        }))
    }

    async fn change_global_option(&self, options: serde_json::Value) -> RpcResult<String> {
        if let Some(limit) = options.get("max-overall-download-limit").and_then(|v| v.as_str()) {
            if let Ok(bytes) = limit.parse::<u64>() {
                self.engine.global_rate_limiter.update_limit(bytes);
                debug!(limit, bytes, "changed global download limit");
            }
        }
        if let Some(max) = options.get("max-concurrent-downloads").and_then(|v| v.as_str()) {
            if let Ok(n) = max.parse::<u32>() {
                self.engine.scheduler.set_max_concurrent(n);
                self.engine.work_notify().notify_one();
                debug!(max = n, "changed max concurrent downloads");
            }
        }
        Ok("OK".into())
    }

    async fn get_global_option(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!({
            "dir": self.engine.config.dir.to_string_lossy(),
            "max-concurrent-downloads": self.engine.scheduler.max_concurrent().to_string(),
            "max-overall-download-limit": self.engine.global_rate_limiter.limit_bps().to_string(),
            "max-overall-upload-limit": self.engine.config.max_overall_upload_limit.to_string(),
            "log-level": self.engine.config.log_level,
            "all-proxy": self.engine.config.all_proxy.as_deref().unwrap_or(""),
            "http-proxy": self.engine.config.http_proxy.as_deref().unwrap_or(""),
            "https-proxy": self.engine.config.https_proxy.as_deref().unwrap_or(""),
            "no-proxy": self.engine.config.no_proxy.as_deref().unwrap_or(""),
            "check-certificate": if self.engine.config.check_certificate { "true" } else { "false" },
            "user-agent": self.engine.config.user_agent.as_deref().unwrap_or(concat!("raria/", env!("CARGO_PKG_VERSION"))),
        }))
    }

    async fn change_position(
        &self,
        gid: String,
        pos: i32,
        how: String,
    ) -> RpcResult<i64> {
        let parsed_gid = parse_gid(&gid)?;
        let position_how = match how.as_str() {
            "POS_SET" => PositionHow::Set,
            "POS_CUR" => PositionHow::Cur,
            "POS_END" => PositionHow::End,
            _ => return Err(rpc_err(1, &format!("unknown position mode: {how}"))),
        };
        let new_pos = self
            .engine
            .change_position(parsed_gid, pos, position_how)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(new_pos as i64)
    }

    // ── Session management ───────────────────────────────────────────

    async fn purge_download_result(&self) -> RpcResult<String> {
        self.engine.purge_download_results();
        Ok("OK".into())
    }

    async fn remove_download_result(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine
            .remove_download_result(parsed_gid)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok("OK".into())
    }

    async fn save_session(&self) -> RpcResult<String> {
        self.engine
            .save_session()
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok("OK".into())
    }

    async fn shutdown(&self) -> RpcResult<String> {
        info!("RPC shutdown requested");
        self.engine.shutdown();
        Ok("OK".into())
    }

    async fn force_shutdown(&self) -> RpcResult<String> {
        info!("RPC force shutdown requested");
        self.engine.shutdown();
        Ok("OK".into())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn parse_gid(gid: &str) -> RpcResult<raria_core::job::Gid> {
    let raw = u64::from_str_radix(gid, 16).map_err(|_| rpc_err(1, &format!("invalid GID: {gid}")))?;
    Ok(raria_core::job::Gid::from_raw(raw))
}

fn gid_not_found(gid: &str) -> jsonrpsee::types::ErrorObjectOwned {
    rpc_err(1, &format!("GID {gid} is not found"))
}

fn internal_error(msg: &str) -> jsonrpsee::types::ErrorObjectOwned {
    jsonrpsee::types::ErrorObjectOwned::owned(-32603, msg.to_string(), None::<()>)
}

fn rpc_err(code: i32, msg: &str) -> jsonrpsee::types::ErrorObjectOwned {
    jsonrpsee::types::ErrorObjectOwned::owned(code, msg.to_string(), None::<()>)
}

fn parse_select_file_spec(spec: &str) -> anyhow::Result<Vec<usize>> {
    let mut result = Vec::new();
    for part in spec.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let one_based: usize = trimmed
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid select-file entry: {trimmed}"))?;
        anyhow::ensure!(one_based > 0, "select-file indices must be 1-based");
        result.push(one_based - 1);
    }
    anyhow::ensure!(!result.is_empty(), "select-file must contain at least one index");
    Ok(result)
}

fn jobs_to_json(jobs: &[raria_core::job::Job]) -> RpcResult<Vec<serde_json::Value>> {
    jobs.iter()
        .map(|j| {
            let status = facade::job_to_aria2_status(j);
            serde_json::to_value(&status).map_err(|e| internal_error(&e.to_string()))
        })
        .collect()
}

fn apply_offset_limit(jobs: &mut Vec<raria_core::job::Job>, offset: i64, num: u32) {
    let start = if offset >= 0 {
        offset as usize
    } else {
        jobs.len().saturating_sub((-offset) as usize)
    };
    if start >= jobs.len() {
        jobs.clear();
        return;
    }
    *jobs = jobs[start..].to_vec();
    jobs.truncate(num as usize);
}

#[cfg(test)]
mod tests {
    use super::*;
    use raria_core::config::GlobalConfig;
    use raria_core::job::{BtFile, Job};
    use std::path::PathBuf;

    fn test_engine() -> Arc<Engine> {
        Arc::new(Engine::new(GlobalConfig::default()))
    }

    #[test]
    fn parse_gid_valid_hex() {
        let gid = parse_gid("00000000000000ff").unwrap();
        assert_eq!(gid.as_raw(), 255);
    }

    #[test]
    fn parse_gid_invalid_hex() {
        assert!(parse_gid("not_hex").is_err());
    }

    #[test]
    fn parse_gid_zero() {
        let gid = parse_gid("0000000000000000").unwrap();
        assert_eq!(gid.as_raw(), 0);
    }

    #[tokio::test]
    async fn add_uri_and_tell_status() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/file.zip".into()], None)
            .await
            .unwrap();

        let status = handler.tell_status(gid_str.clone()).await.unwrap();
        assert_eq!(status["status"], "waiting");
        assert!(!status["files"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn add_uri_with_options() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let opts = RpcOptions {
            dir: Some("/tmp/custom".into()),
            filename: Some("my.zip".into()),
            connections: Some("4".into()),
            ..Default::default()
        };

        let gid_str = handler
            .add_uri(vec!["https://example.com/file.zip".into()], Some(opts))
            .await
            .unwrap();

        let status = handler.tell_status(gid_str).await.unwrap();
        assert!(status["files"][0]["path"].as_str().unwrap().contains("my.zip"));
    }

    #[tokio::test]
    async fn tell_status_gid_not_found() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        assert!(handler.tell_status("00000000deadbeef".into()).await.is_err());
    }

    #[tokio::test]
    async fn get_global_stat_empty() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        let stat = handler.get_global_stat().await.unwrap();
        assert_eq!(stat["numActive"], "0");
        assert_eq!(stat["numWaiting"], "0");
        assert_eq!(stat["numStopped"], "0");
    }

    #[tokio::test]
    async fn tell_active_empty() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        assert!(handler.tell_active().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_version_returns_info() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        let version = handler.get_version().await.unwrap();
        assert!(version["version"].is_string());
        assert!(version["enabledFeatures"].is_array());
    }

    #[tokio::test]
    async fn pause_and_unpause_roundtrip() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f.zip".into()], None)
            .await
            .unwrap();

        let parsed_gid = parse_gid(&gid_str).unwrap();
        engine.activate_job(parsed_gid).unwrap();

        handler.pause(gid_str.clone()).await.unwrap();
        let status = handler.tell_status(gid_str.clone()).await.unwrap();
        assert_eq!(status["status"], "paused");

        handler.unpause(gid_str.clone()).await.unwrap();
        let status = handler.tell_status(gid_str).await.unwrap();
        assert_eq!(status["status"], "waiting");
    }

    #[tokio::test]
    async fn remove_job_via_rpc() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f.zip".into()], None)
            .await
            .unwrap();

        let parsed_gid = parse_gid(&gid_str).unwrap();
        engine.activate_job(parsed_gid).unwrap();

        handler.remove(gid_str.clone()).await.unwrap();
        let status = handler.tell_status(gid_str).await.unwrap();
        assert_eq!(status["status"], "removed");
    }

    #[tokio::test]
    async fn tell_waiting_with_offset() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        handler.add_uri(vec!["https://a.com/1".into()], None).await.unwrap();
        handler.add_uri(vec!["https://a.com/2".into()], None).await.unwrap();
        handler.add_uri(vec!["https://a.com/3".into()], None).await.unwrap();

        let waiting = handler.tell_waiting(0, 2).await.unwrap();
        assert_eq!(waiting.len(), 2);

        let waiting = handler.tell_waiting(1, 10).await.unwrap();
        assert_eq!(waiting.len(), 2);
    }

    #[test]
    fn apply_offset_limit_basic() {
        let mut jobs = vec![
            raria_core::job::Job::new_range(vec!["a".into()], PathBuf::from("/a")),
            raria_core::job::Job::new_range(vec!["b".into()], PathBuf::from("/b")),
            raria_core::job::Job::new_range(vec!["c".into()], PathBuf::from("/c")),
        ];
        apply_offset_limit(&mut jobs, 1, 1);
        assert_eq!(jobs.len(), 1);
    }

    #[test]
    fn apply_offset_limit_negative() {
        let mut jobs = vec![
            raria_core::job::Job::new_range(vec!["a".into()], PathBuf::from("/a")),
            raria_core::job::Job::new_range(vec!["b".into()], PathBuf::from("/b")),
            raria_core::job::Job::new_range(vec!["c".into()], PathBuf::from("/c")),
        ];
        apply_offset_limit(&mut jobs, -2, 10);
        assert_eq!(jobs.len(), 2);
    }

    // ── New method tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn pause_all_pauses_everything() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        handler.add_uri(vec!["https://a.com/1".into()], None).await.unwrap();
        handler.add_uri(vec!["https://a.com/2".into()], None).await.unwrap();

        let result = handler.pause_all().await.unwrap();
        assert_eq!(result, "OK");

        let waiting = handler.tell_waiting(0, 100).await.unwrap();
        // All paused → they show up in waiting (Paused is grouped with Waiting).
        assert_eq!(waiting.len(), 2);
    }

    #[tokio::test]
    async fn unpause_all_resumes_everything() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let g1 = handler.add_uri(vec!["https://a.com/1".into()], None).await.unwrap();
        let g2 = handler.add_uri(vec!["https://a.com/2".into()], None).await.unwrap();

        // Pause via engine to get them into Paused.
        engine.activate_job(parse_gid(&g1).unwrap()).unwrap();
        engine.activate_job(parse_gid(&g2).unwrap()).unwrap();
        engine.pause(parse_gid(&g1).unwrap()).unwrap();
        engine.pause(parse_gid(&g2).unwrap()).unwrap();

        handler.unpause_all().await.unwrap();

        let stat = handler.get_global_stat().await.unwrap();
        assert_eq!(stat["numWaiting"], "2");
    }

    #[tokio::test]
    async fn force_remove_via_rpc() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f.zip".into()], None)
            .await
            .unwrap();

        let parsed_gid = parse_gid(&gid_str).unwrap();
        engine.activate_job(parsed_gid).unwrap();

        handler.force_remove(gid_str.clone()).await.unwrap();
        let status = handler.tell_status(gid_str).await.unwrap();
        assert_eq!(status["status"], "removed");
    }

    #[tokio::test]
    async fn get_uris_returns_job_uris() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(
                vec!["https://a.com/f".into(), "https://b.com/f".into()],
                None,
            )
            .await
            .unwrap();

        let uris = handler.get_uris(gid_str).await.unwrap();
        assert_eq!(uris.len(), 2);
        assert_eq!(uris[0]["uri"], "https://a.com/f");
        assert_eq!(uris[0]["status"], "used");
    }

    #[tokio::test]
    async fn get_files_returns_file_info() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/data.zip".into()], None)
            .await
            .unwrap();

        let files = handler.get_files(gid_str).await.unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0]["path"].as_str().unwrap().contains("data.zip"));
    }

    #[tokio::test]
    async fn get_files_returns_bt_file_entries_when_metadata_is_available() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:abc123".into()],
            PathBuf::from("/tmp/bt-download"),
        );
        job.bt_files = Some(vec![
            BtFile {
                index: 0,
                path: PathBuf::from("disc1/file-a.bin"),
                length: 100,
                completed_length: 25,
                selected: true,
            },
            BtFile {
                index: 1,
                path: PathBuf::from("disc1/file-b.bin"),
                length: 200,
                completed_length: 0,
                selected: false,
            },
        ]);
        let gid = job.gid;
        engine.registry.insert(job).unwrap();

        let files = handler.get_files(gid.to_string()).await.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["index"], "1");
        assert_eq!(files[0]["path"], "disc1/file-a.bin");
        assert_eq!(files[0]["length"], "100");
        assert_eq!(files[0]["completedLength"], "25");
        assert_eq!(files[0]["selected"], "true");

        assert_eq!(files[1]["index"], "2");
        assert_eq!(files[1]["path"], "disc1/file-b.bin");
        assert_eq!(files[1]["selected"], "false");
    }

    #[tokio::test]
    async fn get_peers_returns_empty_for_http() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f".into()], None)
            .await
            .unwrap();

        let peers = handler.get_peers(gid_str).await.unwrap();
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn get_servers_returns_server_info() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f".into()], None)
            .await
            .unwrap();

        let servers = handler.get_servers(gid_str).await.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["index"], "1");
    }

    #[tokio::test]
    async fn get_session_info_returns_session_id() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        let info = handler.get_session_info().await.unwrap();
        assert!(info["sessionId"].is_string());
    }

    #[tokio::test]
    async fn get_global_option_returns_config() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        let opts = handler.get_global_option().await.unwrap();
        assert!(opts["max-concurrent-downloads"].is_string());
        assert!(opts["dir"].is_string());
    }

    #[tokio::test]
    async fn get_option_returns_job_options() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f".into()], None)
            .await
            .unwrap();

        let opts = handler.get_option(gid_str).await.unwrap();
        assert!(opts["dir"].is_string());
        assert!(opts["out"].is_string());
    }

    #[tokio::test]
    async fn change_position_via_rpc() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let _g1 = handler.add_uri(vec!["https://a.com/1".into()], None).await.unwrap();
        let _g2 = handler.add_uri(vec!["https://a.com/2".into()], None).await.unwrap();
        let g3 = handler.add_uri(vec!["https://a.com/3".into()], None).await.unwrap();

        let new_pos = handler.change_position(g3, 0, "POS_SET".into()).await.unwrap();
        assert_eq!(new_pos, 0);
    }

    #[tokio::test]
    async fn change_position_invalid_mode() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let g1 = handler.add_uri(vec!["https://a.com/1".into()], None).await.unwrap();
        assert!(handler.change_position(g1, 0, "INVALID".into()).await.is_err());
    }

    #[tokio::test]
    async fn purge_download_result_via_rpc() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f".into()], None)
            .await
            .unwrap();
        let parsed_gid = parse_gid(&gid_str).unwrap();
        engine.activate_job(parsed_gid).unwrap();
        engine.complete_job(parsed_gid).unwrap();

        handler.purge_download_result().await.unwrap();
        assert!(handler.tell_status(gid_str).await.is_err()); // Purged.
    }

    #[tokio::test]
    async fn remove_download_result_via_rpc() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["https://example.com/f".into()], None)
            .await
            .unwrap();
        let parsed_gid = parse_gid(&gid_str).unwrap();
        engine.activate_job(parsed_gid).unwrap();
        engine.complete_job(parsed_gid).unwrap();

        handler.remove_download_result(gid_str.clone()).await.unwrap();
        assert!(handler.tell_status(gid_str).await.is_err());
    }

    #[tokio::test]
    async fn shutdown_via_rpc() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));
        handler.shutdown().await.unwrap();
        assert!(engine.shutdown_token().is_cancelled());
    }

    #[tokio::test]
    async fn add_torrent_returns_error() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        assert!(handler.add_torrent("base64data".into(), None, None).await.is_err());
    }
}
