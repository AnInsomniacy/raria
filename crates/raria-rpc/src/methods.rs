// raria-rpc: JSON-RPC method handlers.
//
// Implements the aria2-compatible JSON-RPC methods using jsonrpsee.
// These methods provide external control of the download engine.
//
// P0 methods implemented:
// - aria2.addUri       — add a new download
// - aria2.tellStatus   — query job status
// - aria2.pause        — pause a job
// - aria2.unpause      — resume a paused job
// - aria2.remove       — remove a job
// - aria2.getGlobalStat — global statistics
// - aria2.tellActive    — list active downloads
// - aria2.tellWaiting   — list waiting downloads
// - aria2.tellStopped   — list stopped downloads
// - aria2.getVersion    — server version information

use crate::facade;
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::job::Status;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error};

/// aria2-style request options (per-download overrides).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RpcOptions {
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default, rename = "out")]
    pub filename: Option<String>,
    #[serde(default, rename = "split")]
    pub connections: Option<String>,
}

/// JSON-RPC interface definition (aria2-compatible subset).
#[rpc(server)]
pub trait Aria2Rpc {
    /// Add a new download.
    ///
    /// Params: [[uris], options]
    /// Returns: GID string
    #[method(name = "aria2.addUri")]
    async fn add_uri(
        &self,
        uris: Vec<String>,
        options: Option<RpcOptions>,
    ) -> RpcResult<String>;

    /// Query the status of a download.
    ///
    /// Params: [gid]
    /// Returns: aria2-compatible status object
    #[method(name = "aria2.tellStatus")]
    async fn tell_status(
        &self,
        gid: String,
    ) -> RpcResult<serde_json::Value>;

    /// Pause a download.
    #[method(name = "aria2.pause")]
    async fn pause(&self, gid: String) -> RpcResult<String>;

    /// Unpause (resume) a paused download.
    #[method(name = "aria2.unpause")]
    async fn unpause(&self, gid: String) -> RpcResult<String>;

    /// Remove a download.
    #[method(name = "aria2.remove")]
    async fn remove(&self, gid: String) -> RpcResult<String>;

    /// Get global statistics.
    #[method(name = "aria2.getGlobalStat")]
    async fn get_global_stat(&self) -> RpcResult<serde_json::Value>;

    /// List active downloads.
    #[method(name = "aria2.tellActive")]
    async fn tell_active(&self) -> RpcResult<Vec<serde_json::Value>>;

    /// List waiting downloads (with offset + num).
    #[method(name = "aria2.tellWaiting")]
    async fn tell_waiting(
        &self,
        offset: i64,
        num: u32,
    ) -> RpcResult<Vec<serde_json::Value>>;

    /// List stopped downloads (with offset + num).
    #[method(name = "aria2.tellStopped")]
    async fn tell_stopped(
        &self,
        offset: i64,
        num: u32,
    ) -> RpcResult<Vec<serde_json::Value>>;

    /// Get version information.
    #[method(name = "aria2.getVersion")]
    async fn get_version(&self) -> RpcResult<serde_json::Value>;
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
            .map_err(|e| {
                error!(error = %e, "RPC addUri failed");
                jsonrpsee::types::ErrorObjectOwned::owned(
                    1,
                    e.to_string(),
                    None::<()>,
                )
            })?;

        debug!(gid = %handle.gid, "RPC addUri succeeded");
        Ok(format!("{}", handle.gid))
    }

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

    async fn pause(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine
            .pause(parsed_gid)
            .map_err(|e| {
                jsonrpsee::types::ErrorObjectOwned::owned(1, e.to_string(), None::<()>)
            })?;
        Ok(gid)
    }

    async fn unpause(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine
            .unpause(parsed_gid)
            .map_err(|e| {
                jsonrpsee::types::ErrorObjectOwned::owned(1, e.to_string(), None::<()>)
            })?;
        Ok(gid)
    }

    async fn remove(&self, gid: String) -> RpcResult<String> {
        let parsed_gid = parse_gid(&gid)?;
        self.engine
            .remove(parsed_gid)
            .map_err(|e| {
                jsonrpsee::types::ErrorObjectOwned::owned(1, e.to_string(), None::<()>)
            })?;
        Ok(gid)
    }

    async fn get_global_stat(&self) -> RpcResult<serde_json::Value> {
        let jobs = self.engine.registry.snapshot();
        let stat = facade::compute_global_stat(&jobs);
        serde_json::to_value(&stat).map_err(|e| internal_error(&e.to_string()))
    }

    async fn tell_active(&self) -> RpcResult<Vec<serde_json::Value>> {
        let jobs = self.engine.registry.by_status(Status::Active);
        jobs_to_json(&jobs)
    }

    async fn tell_waiting(
        &self,
        offset: i64,
        num: u32,
    ) -> RpcResult<Vec<serde_json::Value>> {
        let mut jobs = self.engine.registry.by_status(Status::Waiting);
        jobs.extend(self.engine.registry.by_status(Status::Paused));
        apply_offset_limit(&mut jobs, offset, num);
        jobs_to_json(&jobs)
    }

    async fn tell_stopped(
        &self,
        offset: i64,
        num: u32,
    ) -> RpcResult<Vec<serde_json::Value>> {
        let mut jobs = self.engine.registry.by_status(Status::Complete);
        jobs.extend(self.engine.registry.by_status(Status::Error));
        jobs.extend(self.engine.registry.by_status(Status::Removed));
        apply_offset_limit(&mut jobs, offset, num);
        jobs_to_json(&jobs)
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
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Parse a hex GID string into a Gid.
fn parse_gid(gid: &str) -> RpcResult<raria_core::job::Gid> {
    let raw = u64::from_str_radix(gid, 16).map_err(|_| {
        jsonrpsee::types::ErrorObjectOwned::owned(
            1,
            format!("invalid GID: {gid}"),
            None::<()>,
        )
    })?;
    Ok(raria_core::job::Gid::from_raw(raw))
}

/// Build a "GID not found" error.
fn gid_not_found(gid: &str) -> jsonrpsee::types::ErrorObjectOwned {
    jsonrpsee::types::ErrorObjectOwned::owned(
        1,
        format!("GID {gid} is not found"),
        None::<()>,
    )
}

/// Build an internal error.
fn internal_error(msg: &str) -> jsonrpsee::types::ErrorObjectOwned {
    jsonrpsee::types::ErrorObjectOwned::owned(-32603, msg.to_string(), None::<()>)
}

/// Convert jobs to JSON array of aria2 status objects.
fn jobs_to_json(jobs: &[raria_core::job::Job]) -> RpcResult<Vec<serde_json::Value>> {
    jobs.iter()
        .map(|j| {
            let status = facade::job_to_aria2_status(j);
            serde_json::to_value(&status).map_err(|e| internal_error(&e.to_string()))
        })
        .collect()
}

/// Apply offset/limit to a mutable Vec (aria2-style).
fn apply_offset_limit(jobs: &mut Vec<raria_core::job::Job>, offset: i64, num: u32) {
    let start = if offset >= 0 {
        offset as usize
    } else {
        // Negative offset means from the end.
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
        let result = parse_gid("not_hex");
        assert!(result.is_err());
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
        assert!(status["files"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn add_uri_with_options() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        let opts = RpcOptions {
            dir: Some("/tmp/custom".into()),
            filename: Some("my.zip".into()),
            connections: Some("4".into()),
        };

        let gid_str = handler
            .add_uri(
                vec!["https://example.com/file.zip".into()],
                Some(opts),
            )
            .await
            .unwrap();

        let status = handler.tell_status(gid_str).await.unwrap();
        assert!(status["files"][0]["path"]
            .as_str()
            .unwrap()
            .contains("my.zip"));
    }

    #[tokio::test]
    async fn tell_status_gid_not_found() {
        let engine = test_engine();
        let handler = RpcHandler::new(engine);

        let result = handler.tell_status("00000000deadbeef".into()).await;
        assert!(result.is_err());
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

        let active = handler.tell_active().await.unwrap();
        assert!(active.is_empty());
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

        // Activate so we can pause.
        let parsed_gid = parse_gid(&gid_str).unwrap();
        engine.activate_job(parsed_gid).unwrap();

        // Pause.
        let result = handler.pause(gid_str.clone()).await;
        assert!(result.is_ok());

        let status = handler.tell_status(gid_str.clone()).await.unwrap();
        assert_eq!(status["status"], "paused");

        // Unpause.
        let result = handler.unpause(gid_str.clone()).await;
        assert!(result.is_ok());

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

        let result = handler.remove(gid_str.clone()).await;
        assert!(result.is_ok());

        let status = handler.tell_status(gid_str).await.unwrap();
        assert_eq!(status["status"], "removed");
    }

    #[tokio::test]
    async fn tell_waiting_with_offset() {
        let engine = test_engine();
        let handler = RpcHandler::new(Arc::clone(&engine));

        // Add 3 downloads.
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

        // Get first 2.
        let waiting = handler.tell_waiting(0, 2).await.unwrap();
        assert_eq!(waiting.len(), 2);

        // Get with offset 1.
        let waiting = handler.tell_waiting(1, 10).await.unwrap();
        assert_eq!(waiting.len(), 2); // 3 total, skip 1 = 2 remaining.
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
        // Negative offset: last 2.
        apply_offset_limit(&mut jobs, -2, 10);
        assert_eq!(jobs.len(), 2);
    }
}
