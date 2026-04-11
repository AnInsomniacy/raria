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
    /// Override download directory.
    #[serde(default)]
    pub dir: Option<String>,
    /// Override output filename.
    #[serde(default, rename = "out")]
    pub filename: Option<String>,
    /// Number of parallel connections (string, e.g. `"4"`).
    #[serde(default, rename = "split")]
    pub connections: Option<String>,
    /// Per-download speed limit in bytes/sec (string).
    #[serde(default, rename = "max-download-limit")]
    pub max_download_limit: Option<String>,
    /// Additional HTTP headers (`Name: Value` format).
    #[serde(default, rename = "header")]
    pub header: Option<Vec<String>>,
    /// Expected checksum in `algo=hex` format (e.g. `sha-256=abc123`).
    #[serde(default, rename = "checksum")]
    pub checksum: Option<String>,
    /// HTTP basic auth username.
    #[serde(default, rename = "http-user")]
    pub http_user: Option<String>,
    /// HTTP basic auth password.
    #[serde(default, rename = "http-passwd")]
    pub http_passwd: Option<String>,
    /// Comma-separated file indices to download (BT only).
    #[serde(default, rename = "select-file")]
    pub select_file: Option<String>,
    /// Additional BT tracker URLs (comma-separated).
    #[serde(default, rename = "bt-tracker")]
    pub bt_tracker: Option<String>,
    /// Stop seeding after reaching this ratio (e.g. `"1.0"`).
    #[serde(default, rename = "seed-ratio")]
    pub seed_ratio: Option<String>,
    /// Stop seeding after this many minutes.
    #[serde(default, rename = "seed-time")]
    pub seed_time: Option<String>,
}

/// JSON-RPC interface definition — full aria2 parity.
#[rpc(server)]
pub trait Aria2Rpc {
    // ── Download control ─────────────────────────────────────────────

    /// Add a download by URI(s). Returns the GID.
    #[method(name = "aria2.addUri")]
    async fn add_uri(&self, uris: Vec<String>, options: Option<RpcOptions>) -> RpcResult<String>;

    /// Add a download by base64-encoded `.torrent` file. Returns the GID.
    #[method(name = "aria2.addTorrent")]
    async fn add_torrent(
        &self,
        torrent_base64: String,
        uris: Option<Vec<String>>,
        options: Option<RpcOptions>,
    ) -> RpcResult<String>;

    /// Add downloads from a base64-encoded `.metalink` file. Returns GIDs.
    #[method(name = "aria2.addMetalink")]
    async fn add_metalink(
        &self,
        metalink_base64: String,
        options: Option<RpcOptions>,
    ) -> RpcResult<Vec<String>>;

    /// Remove a download. Returns the GID.
    #[method(name = "aria2.remove")]
    async fn remove(&self, gid: String) -> RpcResult<String>;

    /// Forcefully remove a download (no graceful teardown).
    #[method(name = "aria2.forceRemove")]
    async fn force_remove(&self, gid: String) -> RpcResult<String>;

    /// Pause a download. Returns the GID.
    #[method(name = "aria2.pause")]
    async fn pause(&self, gid: String) -> RpcResult<String>;

    /// Pause all active/waiting downloads.
    #[method(name = "aria2.pauseAll")]
    async fn pause_all(&self) -> RpcResult<String>;

    /// Forcefully pause a download.
    #[method(name = "aria2.forcePause")]
    async fn force_pause(&self, gid: String) -> RpcResult<String>;

    /// Forcefully pause all downloads.
    #[method(name = "aria2.forcePauseAll")]
    async fn force_pause_all(&self) -> RpcResult<String>;

    /// Resume a paused download. Returns the GID.
    #[method(name = "aria2.unpause")]
    async fn unpause(&self, gid: String) -> RpcResult<String>;

    /// Resume all paused downloads.
    #[method(name = "aria2.unpauseAll")]
    async fn unpause_all(&self) -> RpcResult<String>;

    // ── Query ────────────────────────────────────────────────────────

    /// Get status of a download by GID.
    #[method(name = "aria2.tellStatus")]
    async fn tell_status(&self, gid: String) -> RpcResult<serde_json::Value>;

    /// Get URIs associated with a download.
    #[method(name = "aria2.getUris")]
    async fn get_uris(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    /// Get file information for a download.
    #[method(name = "aria2.getFiles")]
    async fn get_files(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    /// Get BT peer list for a download.
    #[method(name = "aria2.getPeers")]
    async fn get_peers(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    /// Get server information for a download.
    #[method(name = "aria2.getServers")]
    async fn get_servers(&self, gid: String) -> RpcResult<Vec<serde_json::Value>>;

    /// List all active downloads.
    #[method(name = "aria2.tellActive")]
    async fn tell_active(&self) -> RpcResult<Vec<serde_json::Value>>;

    /// List waiting downloads (paginated by offset and count).
    #[method(name = "aria2.tellWaiting")]
    async fn tell_waiting(&self, offset: i64, num: u32) -> RpcResult<Vec<serde_json::Value>>;

    /// List stopped downloads (paginated by offset and count).
    #[method(name = "aria2.tellStopped")]
    async fn tell_stopped(&self, offset: i64, num: u32) -> RpcResult<Vec<serde_json::Value>>;

    /// Get global download/upload statistics.
    #[method(name = "aria2.getGlobalStat")]
    async fn get_global_stat(&self) -> RpcResult<serde_json::Value>;

    /// Get raria version information.
    #[method(name = "aria2.getVersion")]
    async fn get_version(&self) -> RpcResult<serde_json::Value>;

    /// Get current session information.
    #[method(name = "aria2.getSessionInfo")]
    async fn get_session_info(&self) -> RpcResult<serde_json::Value>;

    // ── Configuration ────────────────────────────────────────────────

    /// Change per-download options at runtime.
    #[method(name = "aria2.changeOption")]
    async fn change_option(&self, gid: String, options: serde_json::Value) -> RpcResult<String>;

    /// Get per-download options.
    #[method(name = "aria2.getOption")]
    async fn get_option(&self, gid: String) -> RpcResult<serde_json::Value>;

    /// Change global options at runtime.
    #[method(name = "aria2.changeGlobalOption")]
    async fn change_global_option(&self, options: serde_json::Value) -> RpcResult<String>;

    /// Get global options.
    #[method(name = "aria2.getGlobalOption")]
    async fn get_global_option(&self) -> RpcResult<serde_json::Value>;

    /// Change queue position of a download.
    #[method(name = "aria2.changePosition")]
    async fn change_position(&self, gid: String, pos: i32, how: String) -> RpcResult<i64>;

    // ── Session management ───────────────────────────────────────────

    /// Remove all completed/failed/removed downloads from memory.
    #[method(name = "aria2.purgeDownloadResult")]
    async fn purge_download_result(&self) -> RpcResult<String>;

    /// Remove a single completed/failed download from memory.
    #[method(name = "aria2.removeDownloadResult")]
    async fn remove_download_result(&self, gid: String) -> RpcResult<String>;

    /// Persist current session state to disk.
    #[method(name = "aria2.saveSession")]
    async fn save_session(&self) -> RpcResult<String>;

    /// Gracefully shut down the daemon.
    #[method(name = "aria2.shutdown")]
    async fn shutdown(&self) -> RpcResult<String>;

    /// Forcefully shut down the daemon.
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

    async fn add_uri(&self, uris: Vec<String>, options: Option<RpcOptions>) -> RpcResult<String> {
        let opts = options.unwrap_or_default();
        let dir = opts
            .dir
            .clone()
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
            filename: opts.filename.clone(),
            connections,
        };

        let handle = self
            .engine
            .add_uri(&spec)
            .map_err(|e| rpc_err(1, &e.to_string()))?;

        // Apply per-job options from RPC request.
        let gid = handle.gid;
        self.engine.registry.update(gid, |job| {
            apply_common_rpc_job_options(job, &opts);
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
            .as_ref()
            .and_then(|opts| opts.select_file.as_deref())
            .map(parse_select_file_spec)
            .transpose()
            .map_err(|e| rpc_err(1, &e.to_string()))?
        {
            job.options.bt_selected_files = Some(select_file);
        }
        if let Some(trackers) = options
            .as_ref()
            .and_then(|opts| opts.bt_tracker.as_deref())
            .map(parse_bt_tracker_spec)
            .transpose()
            .map_err(|e| rpc_err(1, &e.to_string()))?
        {
            job.options.bt_trackers = Some(trackers);
        }
        if let Some(seed_ratio) = options
            .as_ref()
            .and_then(|opts| opts.seed_ratio.as_deref())
            .and_then(|v| v.parse::<f64>().ok())
        {
            job.options.seed_ratio = Some(seed_ratio);
        }
        if let Some(seed_time) = options
            .as_ref()
            .and_then(|opts| opts.seed_time.as_deref())
            .and_then(|v| v.parse::<u64>().ok())
        {
            job.options.seed_time = Some(seed_time);
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
        options: Option<RpcOptions>,
    ) -> RpcResult<Vec<String>> {
        use base64::Engine as Base64Engine;
        use raria_core::engine::AddUriSpec;
        use raria_metalink::normalizer::{NormalizeOptions, normalize};
        use raria_metalink::parser::parse_metalink;
        let opts = options.unwrap_or_default();

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

        let seeds = normalize(&metalink, &NormalizeOptions::default());

        // Create a job for each file in the metalink.
        let mut gids = Vec::new();
        for seed in seeds {
            if seed.uris.is_empty() {
                continue;
            }

            let dir = opts
                .dir
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.engine.config.dir.clone());
            let connections = opts
                .connections
                .as_ref()
                .and_then(|s| s.parse().ok())
                .unwrap_or(16);
            let spec = AddUriSpec {
                uris: seed.uris.clone(),
                filename: Some(seed.filename.clone()),
                dir,
                connections,
            };

            match self.engine.add_uri(&spec) {
                Ok(handle) => {
                    self.engine.registry.update(handle.gid, |job| {
                        apply_common_rpc_job_options(job, &opts);
                        if let Some(checksum) = seed.checksum.as_ref() {
                            job.options.checksum =
                                Some(format!("{}={}", checksum.algo, checksum.value));
                        }
                        job.total_size = seed.expected_size;
                        job.piece_checksum = seed.piece_checksum.as_ref().map(|piece_checksum| {
                            raria_core::job::PieceChecksum {
                                algo: piece_checksum.algo.clone(),
                                length: piece_checksum.length,
                                hashes: piece_checksum.hashes.clone(),
                            }
                        });
                    });
                    let gid_str = format!("{:016x}", handle.gid.as_raw());
                    debug!(gid = %gid_str, name = %seed.filename, "metalink: added job");
                    gids.push(gid_str);
                }
                Err(e) => {
                    warn!(name = %seed.filename, error = %e, "metalink: failed to add job");
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
        self.engine
            .remove(parsed_gid)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn force_remove(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine
            .force_remove(parsed_gid)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn pause(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine
            .pause(parsed_gid)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
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
        self.engine
            .pause(parsed_gid)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn force_pause_all(&self) -> RpcResult<String> {
        self.engine.pause_all();
        Ok("OK".into())
    }

    async fn unpause(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine
            .unpause(parsed_gid)
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        Ok(gid)
    }

    async fn unpause_all(&self) -> RpcResult<String> {
        self.engine.unpause_all();
        Ok("OK".into())
    }

    // ── Query ────────────────────────────────────────────────────────

    async fn tell_status(&self, gid: String) -> RpcResult<serde_json::Value> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self
            .engine
            .registry
            .get(parsed_gid)
            .ok_or_else(|| gid_not_found(&gid))?;
        let status = facade::job_to_aria2_status(&job);
        serde_json::to_value(&status).map_err(|e| internal_error(&e.to_string()))
    }

    async fn get_uris(&self, gid: String) -> RpcResult<Vec<serde_json::Value>> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self
            .engine
            .registry
            .get(parsed_gid)
            .ok_or_else(|| gid_not_found(&gid))?;
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
        let job = self
            .engine
            .registry
            .get(parsed_gid)
            .ok_or_else(|| gid_not_found(&gid))?;
        let status = facade::job_to_aria2_status(&job);
        let files: Vec<serde_json::Value> = status
            .files
            .iter()
            .map(|f| serde_json::to_value(f).unwrap_or_default())
            .collect();
        Ok(files)
    }

    async fn get_peers(&self, gid: String) -> RpcResult<Vec<serde_json::Value>> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self
            .engine
            .registry
            .get(parsed_gid)
            .ok_or_else(|| gid_not_found(&gid))?;
        if job.kind != raria_core::job::JobKind::Bt {
            return Ok(vec![]);
        }

        let peers = job
            .bt_peers
            .as_ref()
            .map(|peers| {
                peers
                    .iter()
                    .map(|peer| {
                        serde_json::json!({
                            "peerId": "",
                            "ip": peer.ip,
                            "port": peer.port.to_string(),
                            "bitfield": "",
                            "amChoking": "false",
                            "peerChoking": "false",
                            "downloadSpeed": peer.download_speed.to_string(),
                            "uploadSpeed": peer.upload_speed.to_string(),
                            "seeder": if peer.seeder { "true" } else { "false" },
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(peers)
    }

    async fn get_servers(&self, gid: String) -> RpcResult<Vec<serde_json::Value>> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self
            .engine
            .registry
            .get(parsed_gid)
            .ok_or_else(|| gid_not_found(&gid))?;
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
        let mut jobs = self.engine.registry.by_status(Status::Active);
        jobs.extend(self.engine.registry.by_status(Status::Seeding));
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

    async fn change_option(&self, gid: String, options: serde_json::Value) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        let _job = self
            .engine
            .registry
            .get(parsed_gid)
            .ok_or_else(|| gid_not_found(&gid))?;
        let select_file = options
            .get("select-file")
            .and_then(|v| v.as_str())
            .map(parse_select_file_spec)
            .transpose()
            .map_err(|e| rpc_err(1, &e.to_string()))?;
        let headers = options
            .get("header")
            .and_then(|v| v.as_array())
            .map(|headers| {
                headers
                    .iter()
                    .filter_map(|value| value.as_str())
                    .filter_map(|header| {
                        header
                            .split_once(':')
                            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
                    })
                    .collect::<Vec<_>>()
            });

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
            if let Some(conns) = options
                .get("max-connection-per-server")
                .and_then(|v| v.as_str())
            {
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
            if let Some(trackers) = options.get("bt-tracker").and_then(|v| v.as_str()) {
                if let Ok(parsed) = parse_bt_tracker_spec(trackers) {
                    job.options.bt_trackers = Some(parsed);
                    debug!(%gid, "changed bt-tracker");
                }
            }
            if let Some(headers) = headers.clone() {
                job.options.headers = headers;
                debug!(%gid, "changed header");
            }
            if let Some(checksum) = options.get("checksum").and_then(|v| v.as_str()) {
                job.options.checksum = Some(checksum.to_string());
                debug!(%gid, "changed checksum");
            }
            if let Some(user) = options.get("http-user").and_then(|v| v.as_str()) {
                job.options.http_user = Some(user.to_string());
                debug!(%gid, "changed http-user");
            }
            if let Some(passwd) = options.get("http-passwd").and_then(|v| v.as_str()) {
                job.options.http_passwd = Some(passwd.to_string());
                debug!(%gid, "changed http-passwd");
            }
            if let Some(files) = select_file.clone() {
                job.options.bt_selected_files = Some(files);
                debug!(%gid, "changed select-file");
            }
            if let Some(ratio) = options.get("seed-ratio").and_then(|v| v.as_str()) {
                if let Ok(parsed) = ratio.parse::<f64>() {
                    job.options.seed_ratio = Some(parsed);
                    debug!(%gid, ratio = parsed, "changed seed-ratio");
                }
            }
            if let Some(minutes) = options.get("seed-time").and_then(|v| v.as_str()) {
                if let Ok(parsed) = minutes.parse::<u64>() {
                    job.options.seed_time = Some(parsed);
                    debug!(%gid, minutes = parsed, "changed seed-time");
                }
            }
        });
        Ok("OK".into())
    }

    async fn get_option(&self, gid: String) -> RpcResult<serde_json::Value> {
        let parsed_gid = parse_gid(&gid)?;
        let job = self
            .engine
            .registry
            .get(parsed_gid)
            .ok_or_else(|| gid_not_found(&gid))?;

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
            "bt-tracker": job.options.bt_trackers.as_ref()
                .map(|trackers| trackers.join(","))
                .unwrap_or_default(),
            "seed-ratio": job.options.seed_ratio
                .map(|ratio| ratio.to_string())
                .unwrap_or_default(),
            "seed-time": job.options.seed_time
                .map(|minutes| minutes.to_string())
                .unwrap_or_default(),
        }))
    }

    async fn change_global_option(&self, options: serde_json::Value) -> RpcResult<String> {
        if let Some(limit) = options
            .get("max-overall-download-limit")
            .or_else(|| options.get("max-download-limit"))
            .and_then(|v| v.as_str())
        {
            if let Ok(bytes) = limit.parse::<u64>() {
                self.engine.global_rate_limiter.update_limit(bytes);
                debug!(limit, bytes, "changed global download limit");
            }
        }
        if let Some(max) = options
            .get("max-concurrent-downloads")
            .and_then(|v| v.as_str())
        {
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

    async fn change_position(&self, gid: String, pos: i32, how: String) -> RpcResult<i64> {
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
    let raw =
        u64::from_str_radix(gid, 16).map_err(|_| rpc_err(1, &format!("invalid GID: {gid}")))?;
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
    anyhow::ensure!(
        !result.is_empty(),
        "select-file must contain at least one index"
    );
    Ok(result)
}

fn apply_common_rpc_job_options(job: &mut raria_core::job::Job, opts: &RpcOptions) {
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
                job.options
                    .headers
                    .push((key.trim().to_string(), value.trim().to_string()));
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
    if let Some(ref trackers) = opts.bt_tracker {
        if let Ok(trackers) = parse_bt_tracker_spec(trackers) {
            job.options.bt_trackers = Some(trackers);
        }
    }
    if let Some(ref ratio) = opts.seed_ratio {
        if let Ok(ratio) = ratio.parse::<f64>() {
            job.options.seed_ratio = Some(ratio);
        }
    }
    if let Some(ref minutes) = opts.seed_time {
        if let Ok(minutes) = minutes.parse::<u64>() {
            job.options.seed_time = Some(minutes);
        }
    }
}

fn parse_bt_tracker_spec(spec: &str) -> anyhow::Result<Vec<String>> {
    let result = spec
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    anyhow::ensure!(
        !result.is_empty(),
        "bt-tracker must contain at least one tracker"
    );
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
    use raria_core::job::{BtFile, BtPeer, Job};
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
        assert!(
            status["files"][0]["path"]
                .as_str()
                .unwrap()
                .contains("my.zip")
        );
    }

    #[tokio::test]
    async fn tell_status_gid_not_found() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);
        assert!(
            handler
                .tell_status("00000000deadbeef".into())
                .await
                .is_err()
        );
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
    async fn tell_active_includes_seeding_jobs() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));
        let gid = handler
            .add_torrent("bWFnbmV0Oj94dD11cm46YnRpaDphYmM=".into(), None, None)
            .await
            .expect("add_torrent should create bt job");
        let parsed_gid = parse_gid(&gid).unwrap();
        engine
            .registry
            .update(parsed_gid, |job| job.status = Status::Seeding)
            .expect("job should exist");

        let active = handler.tell_active().await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0]["gid"], gid);
        assert_eq!(active[0]["status"], "active");
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

        handler
            .add_uri(vec!["https://a.com/1".into()], None)
            .await
            .unwrap();
        handler
            .add_uri(vec!["https://a.com/2".into()], None)
            .await
            .unwrap();
        handler
            .add_uri(vec!["https://a.com/3".into()], None)
            .await
            .unwrap();

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

        handler
            .add_uri(vec!["https://a.com/1".into()], None)
            .await
            .unwrap();
        handler
            .add_uri(vec!["https://a.com/2".into()], None)
            .await
            .unwrap();

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

        let g1 = handler
            .add_uri(vec!["https://a.com/1".into()], None)
            .await
            .unwrap();
        let g2 = handler
            .add_uri(vec!["https://a.com/2".into()], None)
            .await
            .unwrap();

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
    async fn get_peers_returns_bt_peer_entries_when_cached() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let gid_str = handler
            .add_uri(vec!["magnet:?xt=urn:btih:abc123".into()], None)
            .await
            .unwrap();

        let gid = parse_gid(&gid_str).unwrap();
        engine.registry.update(gid, |job| {
            job.bt_peers = Some(vec![BtPeer {
                addr: "127.0.0.1:6881".into(),
                ip: "127.0.0.1".into(),
                port: 6881,
                download_speed: 512,
                upload_speed: 128,
                seeder: true,
            }]);
        });

        let peers = handler.get_peers(gid_str).await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0]["ip"], "127.0.0.1");
        assert_eq!(peers[0]["port"], "6881");
        assert_eq!(peers[0]["downloadSpeed"], "512");
        assert_eq!(peers[0]["uploadSpeed"], "128");
        assert_eq!(peers[0]["seeder"], "true");
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

        let _g1 = handler
            .add_uri(vec!["https://a.com/1".into()], None)
            .await
            .unwrap();
        let _g2 = handler
            .add_uri(vec!["https://a.com/2".into()], None)
            .await
            .unwrap();
        let g3 = handler
            .add_uri(vec!["https://a.com/3".into()], None)
            .await
            .unwrap();

        let new_pos = handler
            .change_position(g3, 0, "POS_SET".into())
            .await
            .unwrap();
        assert_eq!(new_pos, 0);
    }

    #[tokio::test]
    async fn change_position_invalid_mode() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let g1 = handler
            .add_uri(vec!["https://a.com/1".into()], None)
            .await
            .unwrap();
        assert!(
            handler
                .change_position(g1, 0, "INVALID".into())
                .await
                .is_err()
        );
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

        handler
            .remove_download_result(gid_str.clone())
            .await
            .unwrap();
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
        assert!(
            handler
                .add_torrent("base64data".into(), None, None)
                .await
                .is_err()
        );
    }
}
