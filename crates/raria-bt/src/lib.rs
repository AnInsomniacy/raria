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

pub mod service;
