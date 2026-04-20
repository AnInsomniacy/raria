#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-bt
//!
//! BitTorrent download service for raria, powered by `librqbit`.
//!
//! Provides [`service::BtService`] which bridges the librqbit session
//! lifecycle with raria's job model, offering:
//! - Magnet URI and `.torrent` file support
//! - Lazy session initialization (fast startup when BT unused)
//! - Per-file selection and progress tracking
//! - SOCKS5 proxy passthrough
//! - Fastresume / JSON session persistence
//!
//! ## WebSeed support
//!
//! The [`torrent_meta`] and [`webseed`] modules provide BEP-17/BEP-19
//! WebSeed pre-download capability. Files are downloaded via HTTP/FTP/SFTP
//! *before* librqbit starts, so that its `initial_check` discovers them
//! as already-complete pieces on disk.

/// BitTorrent service, session management, and torrent lifecycle.
pub mod service;

/// Torrent metainfo extraction (file list, piece hashes, WebSeed URIs).
pub mod torrent_meta;

/// WebSeed pre-download service (HTTP/FTP/SFTP via SegmentExecutor).
pub mod webseed;
