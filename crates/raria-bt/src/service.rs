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

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Source for a BitTorrent download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BtSource {
    /// A magnet URI.
    Magnet(String),
    /// Path to a .torrent file.
    TorrentFile(PathBuf),
    /// Raw torrent bytes (e.g., received via RPC).
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
    /// Whether this file is selected for download.
    pub selected: bool,
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
pub struct BtService {
    /// Output directory for downloads.
    output_dir: PathBuf,
    // session: Option<librqbit::Session>,
    // TODO: Initialize librqbit::Session in new().
}

impl BtService {
    /// Create a new BT service.
    pub fn new(output_dir: PathBuf) -> Result<Self> {
        // TODO: Initialize librqbit::Session with SessionOptions.
        Ok(Self {
            output_dir,
        })
    }

    /// Add a new torrent download.
    pub async fn add(&self, _source: BtSource) -> Result<BtHandle> {
        // TODO: Implement using librqbit Session::add_torrent.
        anyhow::bail!("BT service not yet implemented")
    }

    /// Pause a torrent.
    pub async fn pause(&self, _handle: &BtHandle) -> Result<()> {
        anyhow::bail!("BT service not yet implemented")
    }

    /// Resume a paused torrent.
    pub async fn resume(&self, _handle: &BtHandle) -> Result<()> {
        anyhow::bail!("BT service not yet implemented")
    }

    /// Remove a torrent.
    pub async fn remove(&self, _handle: &BtHandle, _delete_files: bool) -> Result<()> {
        anyhow::bail!("BT service not yet implemented")
    }

    /// Get the current status of a torrent.
    pub async fn status(&self, _handle: &BtHandle) -> Result<BtStatus> {
        anyhow::bail!("BT service not yet implemented")
    }

    /// List files in a torrent.
    pub async fn file_list(&self, _handle: &BtHandle) -> Result<Vec<BtFileInfo>> {
        anyhow::bail!("BT service not yet implemented")
    }

    /// Get the output directory.
    pub fn output_dir(&self) -> &PathBuf {
        &self.output_dir
    }
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
    fn bt_status_serde_roundtrips() {
        let status = BtStatus {
            total_size: 1_000_000,
            downloaded: 500_000,
            download_speed: 1024,
            upload_speed: 256,
            num_peers: 10,
            num_seeders: 5,
            is_complete: false,
            info_hash: "abcdef1234567890".into(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let recovered: BtStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.total_size, 1_000_000);
        assert_eq!(recovered.downloaded, 500_000);
        assert_eq!(recovered.info_hash, "abcdef1234567890");
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
            selected: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let recovered: BtFileInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.index, 0);
        assert!(recovered.selected);
    }
}
