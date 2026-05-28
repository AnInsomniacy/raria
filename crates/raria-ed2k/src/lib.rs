#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-ed2k
//!
//! Native ED2K/eMule backend boundary for raria.
//!
//! This crate owns the future Rust implementation of ED2K/eMule protocol
//! behavior. The current checkpoint intentionally exposes only module
//! ownership boundaries and no runtime network behavior.

/// AICH and ED2K hash ownership.
pub mod hash;
/// Stable ED2K client identity ownership.
pub mod identity;
/// Kad routing and search ownership.
pub mod kad;
/// ED2K link parsing ownership.
pub mod link;
/// ED2K peer session ownership.
pub mod peer;
/// ED2K protocol persistence ownership.
pub mod persist;
/// Server TCP and UDP ownership.
pub mod server;
/// Shared-file and upload cooperation ownership.
pub mod sharing;
/// Transfer planning and part payload ownership.
pub mod transfer;
