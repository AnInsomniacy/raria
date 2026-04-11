// raria-bt: BitTorrent service wrapping librqbit.
//
// This module provides BtService — a high-level interface for managing
// BitTorrent downloads. It wraps librqbit's Session API and translates
// between raria's job model and librqbit's internal state.
//
// Key design: BT does NOT go through ByteSourceBackend.
// librqbit manages its own piece scheduling, peer connections, and
// persistence (fastresume). raria-core only manages the GID mapping
// and status aggregation.

use anyhow::{Context, Result};
use librqbit::api::{Api, TorrentIdOrHash};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions,
    SessionPersistenceConfig,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

fn is_selected_file(selected_files: Option<&[usize]>, file_index: usize) -> bool {
    selected_files
        .map(|files| files.contains(&file_index))
        .unwrap_or(true)
}

fn parse_peer_addr(addr: &str) -> (String, u16) {
    if let Some(rest) = addr.strip_prefix('[') {
        if let Some((host, port)) = rest.split_once("]:") {
            if let Ok(port) = port.parse::<u16>() {
                return (host.to_string(), port);
            }
        }
        return (addr.trim_matches(&['[', ']'][..]).to_string(), 0);
    }

    if addr.matches(':').count() == 1 {
        if let Some((host, port)) = addr.split_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                return (host.to_string(), port);
            }
        }
    }

    (addr.to_string(), 0)
}

fn bt_session_persistence_dir(output_dir: &Path) -> PathBuf {
    output_dir.join(".raria-bt-session")
}

fn bt_session_options(output_dir: &Path, config: &BtServiceConfig) -> SessionOptions {
    SessionOptions {
        disable_dht: config.disable_dht,
        disable_dht_persistence: config.disable_dht_persistence,
        dht_config: config.dht_config_filename.as_ref().map(|path| {
            librqbit::dht::PersistentDhtConfig {
                dump_interval: None,
                config_filename: Some(path.clone()),
            }
        }),
        socks_proxy_url: config.socks_proxy_url.clone(),
        fastresume: true,
        persistence: Some(SessionPersistenceConfig::Json {
            folder: Some(bt_session_persistence_dir(output_dir)),
        }),
        ..Default::default()
    }
}

/// Source for a BitTorrent download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BtSource {
    /// A magnet URI.
    Magnet(String),
    /// Path to a .torrent file.
    TorrentFile(PathBuf),
    /// Raw torrent bytes (e.g., received via RPC as base64).
    TorrentBytes(Vec<u8>),
}

/// Handle to a managed BT download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtHandle {
    /// Internal librqbit torrent ID.
    pub torrent_id: usize,
    /// raria GID for cross-referencing.
    pub gid: raria_core::job::Gid,
}

/// Status of a BT download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtStatus {
    /// Total size in bytes.
    pub total_size: u64,
    /// Bytes downloaded.
    pub downloaded: u64,
    /// Bytes uploaded while seeding.
    pub uploaded: u64,
    /// Download speed in bytes/sec.
    pub download_speed: u64,
    /// Upload speed in bytes/sec.
    pub upload_speed: u64,
    /// Number of connected peers.
    pub num_peers: u32,
    /// Number of seeders.
    pub num_seeders: u32,
    /// Whether the download is complete.
    pub is_complete: bool,
    /// Info hash (hex).
    pub info_hash: String,
    /// Torrent display name, when known.
    pub torrent_name: Option<String>,
    /// Best-effort tracker announce list, when known.
    pub announce_list: Option<Vec<String>>,
    /// Piece length in bytes, when known.
    pub piece_length: Option<u64>,
    /// Total number of pieces, when known.
    pub num_pieces: Option<u64>,
}

/// Information about a file within a torrent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtFileInfo {
    /// File index within the torrent.
    pub index: usize,
    /// Relative file path.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Completed bytes for this file, when known.
    pub completed_length: u64,
    /// Whether this file is selected for download.
    pub selected: bool,
}

/// Information about a peer connected to a torrent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtPeerInfo {
    /// Socket address as `ip:port`.
    pub addr: String,
    /// Peer IP address.
    pub ip: String,
    /// Peer port number.
    pub port: u16,
    /// Current download speed from this peer (bytes/sec).
    pub download_speed: u64,
    /// Current upload speed to this peer (bytes/sec).
    pub upload_speed: u64,
    /// `true` if this peer has 100% of the torrent.
    pub seeder: bool,
}

/// BitTorrent download service.
///
/// This is the entry point for all BT operations. It manages a librqbit
/// Session internally and provides raria-compatible operations.
///
/// # Product Constraint
/// librqbit only supports sequential downloading (rarest-first is not available).
/// This means BT download behavior is NOT equivalent to aria2's BT engine.
/// This is an accepted product constraint.
#[derive(Debug, Clone, Default)]
pub struct BtServiceConfig {
    /// SOCKS5 proxy URL for tracker and peer connections.
    pub socks_proxy_url: Option<String>,
    /// Disable DHT node discovery.
    pub disable_dht: bool,
    /// Disable persisting DHT routing table to disk.
    pub disable_dht_persistence: bool,
    /// Custom path for DHT routing table persistence.
    pub dht_config_filename: Option<PathBuf>,
    /// Bootstrap peers to connect to before DHT discovery.
    pub initial_peers: Option<Vec<SocketAddr>>,
}

/// BitTorrent download service managing a librqbit session.
pub struct BtService {
    /// Output directory for downloads.
    output_dir: PathBuf,
    /// BT session runtime config.
    config: BtServiceConfig,
    /// librqbit session (initialized lazily on first use).
    session: Arc<RwLock<Option<Arc<Session>>>>,
    /// Map from raria GID → librqbit `Arc<ManagedTorrent>` for active torrents.
    handles: Arc<RwLock<HashMap<raria_core::job::Gid, Arc<ManagedTorrent>>>>,
}

impl BtService {
    /// Create a new BT service.
    ///
    /// The librqbit session is NOT started here — it's initialized lazily
    /// on the first `add()` call. This keeps startup fast when BT isn't used.
    pub fn new(output_dir: PathBuf) -> Result<Self> {
        Self::with_config(output_dir, BtServiceConfig::default())
    }

    /// Create a BT service with custom configuration.
    pub fn with_config(output_dir: PathBuf, config: BtServiceConfig) -> Result<Self> {
        Ok(Self {
            output_dir,
            config,
            session: Arc::new(RwLock::new(None)),
            handles: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Ensure the librqbit session is initialized. Returns a clone of the Arc.
    async fn ensure_session(&self) -> Result<Arc<Session>> {
        // Fast path: session already initialized.
        {
            let guard = self.session.read();
            if let Some(ref session) = *guard {
                return Ok(Arc::clone(session));
            }
        }

        // Slow path: initialize.
        info!(dir = %self.output_dir.display(), "initializing librqbit session");
        let opts = bt_session_options(&self.output_dir, &self.config);
        let session = Session::new_with_opts(self.output_dir.clone(), opts)
            .await
            .context("failed to initialize librqbit session")?;

        let mut guard = self.session.write();
        *guard = Some(Arc::clone(&session));
        info!("librqbit session initialized");

        Ok(session)
    }

    /// Add a new torrent download.
    pub async fn add(
        &self,
        source: BtSource,
        gid: raria_core::job::Gid,
        selected_files: Option<Vec<usize>>,
        trackers: Option<Vec<String>>,
    ) -> Result<BtHandle> {
        let session = self.ensure_session().await?;

        let add_torrent = match &source {
            BtSource::Magnet(uri) => AddTorrent::Url(uri.into()),
            BtSource::TorrentFile(path) => {
                let bytes = tokio::fs::read(path)
                    .await
                    .with_context(|| format!("failed to read torrent file: {}", path.display()))?;
                AddTorrent::TorrentFileBytes(bytes.into())
            }
            BtSource::TorrentBytes(bytes) => AddTorrent::TorrentFileBytes(bytes.clone().into()),
        };

        let opts = AddTorrentOptions {
            output_folder: Some(self.output_dir.clone().to_string_lossy().to_string()),
            only_files: selected_files,
            initial_peers: self.config.initial_peers.clone(),
            trackers,
            ..Default::default()
        };

        let response = session
            .add_torrent(add_torrent, Some(opts))
            .await
            .context("failed to add torrent")?;

        let (torrent_id, handle) = match response {
            AddTorrentResponse::Added(id, h) => {
                info!(%gid, torrent_id = id, "torrent added");
                (id, h)
            }
            AddTorrentResponse::AlreadyManaged(id, h) => {
                warn!(%gid, torrent_id = id, "torrent already managed");
                (id, h)
            }
            AddTorrentResponse::ListOnly(_) => {
                anyhow::bail!("torrent was list-only, not added for download");
            }
        };

        // Store the handle for later operations.
        self.handles.write().insert(gid, handle);

        Ok(BtHandle { torrent_id, gid })
    }

    /// Pause a torrent.
    pub async fn pause(&self, handle: &BtHandle) -> Result<()> {
        let session = self.ensure_session().await?;
        let managed = self.get_managed_handle(handle)?;
        session
            .pause(&managed)
            .await
            .context("failed to pause torrent")?;
        debug!(gid = %handle.gid, "torrent paused");
        Ok(())
    }

    /// Resume a paused torrent.
    pub async fn resume(&self, handle: &BtHandle) -> Result<()> {
        let session = self.ensure_session().await?;
        let managed = self.get_managed_handle(handle)?;
        session
            .unpause(&managed)
            .await
            .context("failed to unpause torrent")?;
        debug!(gid = %handle.gid, "torrent resumed");
        Ok(())
    }

    /// Remove a torrent.
    pub async fn remove(&self, handle: &BtHandle, delete_files: bool) -> Result<()> {
        let session = self.ensure_session().await?;
        session
            .delete(TorrentIdOrHash::Id(handle.torrent_id), delete_files)
            .await
            .context("failed to remove torrent")?;

        // Remove from our handle map.
        self.handles.write().remove(&handle.gid);
        debug!(gid = %handle.gid, "torrent removed");
        Ok(())
    }

    /// Get the current status of a torrent.
    pub async fn status(&self, handle: &BtHandle) -> Result<BtStatus> {
        let managed = self.get_managed_handle(handle)?;
        let stats = managed.stats();
        let metadata = managed.metadata.load();

        let (download_speed, upload_speed) = if let Some(ref live) = stats.live {
            (
                // Speed is in Mbps (megabits/sec). Convert to bytes/sec:
                // Mbps * 1_000_000 / 8 = Mbps * 125_000
                (live.download_speed.mbps * 125_000.0) as u64,
                (live.upload_speed.mbps * 125_000.0) as u64,
            )
        } else {
            (0, 0)
        };
        let (num_peers, num_seeders) = self
            .live_peer_counts(handle)
            .await
            .unwrap_or_else(|_| {
                stats.live
                    .as_ref()
                    .map(|live| (live.snapshot.peer_stats.live as u32, 0))
                    .unwrap_or((0, 0))
            });
        let torrent_name = managed.name();
        let announce_list = tracker_urls(managed.shared())
            .filter(|trackers| !trackers.is_empty());
        let (piece_length, num_pieces) = managed
            .metadata
            .load()
            .as_ref()
            .map(|metadata| {
                (
                    Some(u64::from(metadata.info.piece_length)),
                    Some(u64::from(metadata.lengths.total_pieces())),
                )
            })
            .unwrap_or((None, None));

        // Id20 is [u8; 20] — format as hex.
        let info_hash_bytes = managed.info_hash();
        let info_hash = hex::encode(info_hash_bytes.0);
        let torrent_name = managed.name();
        let announce_list = {
            let mut trackers = managed
                .shared()
                .trackers
                .iter()
                .map(|tracker| tracker.as_str().to_string())
                .collect::<Vec<_>>();
            trackers.sort();
            trackers.dedup();
            (!trackers.is_empty()).then_some(trackers)
        };
        let (piece_length, num_pieces) = metadata
            .as_ref()
            .map(|metadata| {
                (
                    metadata.info.piece_length as u64,
                    metadata.lengths.total_pieces() as u64,
                )
            })
            .unwrap_or((0, 0));

        Ok(BtStatus {
            total_size: stats.total_bytes,
            downloaded: stats.progress_bytes,
            uploaded: stats.uploaded_bytes,
            download_speed,
            upload_speed,
            num_peers,
            num_seeders,
            is_complete: stats.finished,
            info_hash,
            torrent_name,
            announce_list,
            piece_length,
            num_pieces,
        })
    }

    /// List files in a torrent.
    pub async fn file_list(&self, handle: &BtHandle) -> Result<Vec<BtFileInfo>> {
        let managed = self.get_managed_handle(handle)?;
        let stats = managed.stats();
        // Get file info from the torrent's metadata (loaded via ArcSwap).
        let metadata_guard = managed.metadata.load();
        let metadata = metadata_guard
            .as_ref()
            .context("torrent metadata not yet resolved (magnet still resolving?)")?;
        let selected_files = managed.only_files();

        let files: Vec<BtFileInfo> = metadata
            .file_infos
            .iter()
            .enumerate()
            .map(|(i, fi)| {
                let progress = stats.file_progress.get(i).copied().unwrap_or(0);
                BtFileInfo {
                    index: i,
                    path: fi.relative_filename.clone(),
                    size: fi.len,
                    completed_length: progress,
                    selected: is_selected_file(selected_files.as_deref(), i),
                }
            })
            .collect();

        Ok(files)
    }

    /// List peers in a torrent.
    pub async fn peer_list(&self, handle: &BtHandle) -> Result<Vec<BtPeerInfo>> {
        let session = self.ensure_session().await?;
        let api = Api::new(session, None);
        let snapshot = api
            .api_peer_stats(TorrentIdOrHash::Id(handle.torrent_id), Default::default())
            .context("failed to query BT peer stats")?;

        let peers = snapshot
            .peers
            .into_iter()
            .map(|(addr, stats)| {
                let (ip, port) = parse_peer_addr(&addr);
                let download_speed = if stats.counters.total_piece_download_ms > 0 {
                    stats.counters.fetched_bytes.saturating_mul(1000)
                        / stats.counters.total_piece_download_ms
                } else {
                    0
                };
                BtPeerInfo {
                    addr,
                    ip,
                    port,
                    download_speed,
                    upload_speed: 0,
                    seeder: stats.counters.downloaded_and_checked_pieces > 0,
                }
            })
            .collect();

        Ok(peers)
    }

    /// Get the output directory.
    pub fn output_dir(&self) -> &PathBuf {
        &self.output_dir
    }

    /// Stop the librqbit session.
    pub async fn shutdown(&self) {
        let session = {
            let guard = self.session.read();
            guard.clone()
        };
        if let Some(session) = session {
            session.stop().await;
            info!("librqbit session stopped");
        }
    }

    /// Internal: get the `Arc<ManagedTorrent>` for a BtHandle.
    fn get_managed_handle(&self, handle: &BtHandle) -> Result<Arc<ManagedTorrent>> {
        self.handles
            .read()
            .get(&handle.gid)
            .cloned()
            .with_context(|| format!("no managed handle for GID {}", handle.gid))
    }

    async fn live_peer_counts(&self, handle: &BtHandle) -> Result<(u32, u32)> {
        let session = self.ensure_session().await?;
        let api = Api::new(session, None);
        let snapshot = api
            .api_peer_stats(TorrentIdOrHash::Id(handle.torrent_id), Default::default())
            .context("failed to query BT peer stats")?;

        let num_peers = snapshot.peers.len() as u32;
        let num_seeders = snapshot
            .peers
            .values()
            .filter(|stats| stats.counters.downloaded_and_checked_pieces > 0)
            .count() as u32;
        Ok((num_peers, num_seeders))
    }
}

fn tracker_urls(shared: &librqbit::ManagedTorrentShared) -> Option<Vec<String>> {
    let mut trackers: Vec<String> = shared
        .trackers
        .iter()
        .map(|tracker| tracker.to_string())
        .collect();
    if trackers.is_empty() {
        return None;
    }
    trackers.sort();
    Some(trackers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt_source_magnet_serde_roundtrips() {
        let source = BtSource::Magnet("magnet:?xt=urn:btih:abc123".into());
        let json = serde_json::to_string(&source).unwrap();
        let recovered: BtSource = serde_json::from_str(&json).unwrap();
        match recovered {
            BtSource::Magnet(uri) => assert!(uri.contains("abc123")),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn bt_source_torrent_file_serde() {
        let source = BtSource::TorrentFile(PathBuf::from("/tmp/test.torrent"));
        let json = serde_json::to_string(&source).unwrap();
        let recovered: BtSource = serde_json::from_str(&json).unwrap();
        match recovered {
            BtSource::TorrentFile(path) => assert_eq!(path, PathBuf::from("/tmp/test.torrent")),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn bt_source_torrent_bytes_serde() {
        let source = BtSource::TorrentBytes(vec![1, 2, 3, 4]);
        let json = serde_json::to_string(&source).unwrap();
        let recovered: BtSource = serde_json::from_str(&json).unwrap();
        match recovered {
            BtSource::TorrentBytes(bytes) => assert_eq!(bytes, vec![1, 2, 3, 4]),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn bt_status_serde_roundtrips() {
        let status = BtStatus {
            total_size: 1_000_000,
            downloaded: 500_000,
            uploaded: 123_000,
            download_speed: 1024,
            upload_speed: 256,
            num_peers: 10,
            num_seeders: 5,
            is_complete: false,
            info_hash: "abcdef1234567890".into(),
            torrent_name: Some("fixture.iso".into()),
            announce_list: Some(vec!["http://tracker.example/announce".into()]),
            piece_length: Some(16 * 1024),
            num_pieces: Some(61),
        };
        let json = serde_json::to_string(&status).unwrap();
        let recovered: BtStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.total_size, 1_000_000);
        assert_eq!(recovered.downloaded, 500_000);
        assert_eq!(recovered.uploaded, 123_000);
        assert_eq!(recovered.info_hash, "abcdef1234567890");
        assert_eq!(recovered.torrent_name.as_deref(), Some("fixture.iso"));
        assert_eq!(
            recovered.announce_list.as_ref(),
            Some(&vec!["http://tracker.example/announce".into()])
        );
        assert_eq!(recovered.piece_length, Some(16 * 1024));
        assert_eq!(recovered.num_pieces, Some(61));
    }

    #[test]
    fn bt_service_creates_with_output_dir() {
        let svc = BtService::new(PathBuf::from("/tmp/downloads")).unwrap();
        assert_eq!(svc.output_dir(), &PathBuf::from("/tmp/downloads"));
    }

    #[test]
    fn bt_file_info_serde_roundtrips() {
        let info = BtFileInfo {
            index: 0,
            path: PathBuf::from("subdir/file.txt"),
            size: 42,
            completed_length: 7,
            selected: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let recovered: BtFileInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.index, 0);
        assert_eq!(recovered.completed_length, 7);
        assert!(recovered.selected);
    }

    #[test]
    fn bt_service_session_starts_none() {
        let svc = BtService::new(PathBuf::from("/tmp")).unwrap();
        let guard = svc.session.read();
        assert!(guard.is_none(), "session should be lazy-initialized");
    }

    #[test]
    fn bt_service_handles_starts_empty() {
        let svc = BtService::new(PathBuf::from("/tmp")).unwrap();
        let guard = svc.handles.read();
        assert!(guard.is_empty());
    }

    #[tokio::test]
    async fn bt_service_rejects_non_socks5_proxy_urls_when_session_starts() {
        let svc = BtService::with_config(
            PathBuf::from("/tmp"),
            BtServiceConfig {
                socks_proxy_url: Some("http://127.0.0.1:8080".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let error = match svc.ensure_session().await {
            Ok(_) => panic!("non-socks5 BT proxy must fail"),
            Err(error) => error,
        };
        let error_chain = format!("{error:#}");
        assert!(
            error_chain.contains("proxy") || error_chain.contains("socks5"),
            "unexpected BT proxy error: {error_chain}"
        );
    }

    #[test]
    fn bt_service_session_options_enable_fastresume_and_json_persistence() {
        let output_dir = PathBuf::from("/tmp/raria-bt");
        let options = bt_session_options(
            &output_dir,
            &BtServiceConfig {
                socks_proxy_url: Some("socks5://127.0.0.1:1080".into()),
                disable_dht: true,
                disable_dht_persistence: true,
                dht_config_filename: Some(PathBuf::from("/tmp/raria-bt-dht.json")),
                initial_peers: None,
            },
        );

        assert!(
            options.fastresume,
            "BtService must enable librqbit fastresume"
        );
        assert_eq!(
            options.socks_proxy_url.as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
        assert!(options.disable_dht);
        assert!(options.disable_dht_persistence);
        assert_eq!(
            options
                .dht_config
                .as_ref()
                .and_then(|cfg| cfg.config_filename.as_ref()),
            Some(&PathBuf::from("/tmp/raria-bt-dht.json"))
        );
        match options.persistence {
            Some(SessionPersistenceConfig::Json { folder }) => {
                assert_eq!(folder, Some(bt_session_persistence_dir(&output_dir)));
            }
            _ => panic!("expected JSON persistence config"),
        }
    }

    #[test]
    fn selection_defaults_to_all_files_when_only_files_is_none() {
        assert!(is_selected_file(None, 0));
        assert!(is_selected_file(None, 10));
    }

    #[test]
    fn selection_uses_librqbit_only_files_state_when_present() {
        assert!(is_selected_file(Some(&[0, 2, 4]), 2));
        assert!(!is_selected_file(Some(&[0, 2, 4]), 3));
    }

    #[test]
    fn parse_peer_addr_handles_bracketed_ipv6_with_port() {
        let (ip, port) = parse_peer_addr("[2001:db8::1]:6881");
        assert_eq!(ip, "2001:db8::1");
        assert_eq!(port, 6881);
    }

    #[test]
    fn parse_peer_addr_preserves_raw_ipv6_without_port() {
        let (ip, port) = parse_peer_addr("2001:db8::1");
        assert_eq!(ip, "2001:db8::1");
        assert_eq!(port, 0);
    }
}
