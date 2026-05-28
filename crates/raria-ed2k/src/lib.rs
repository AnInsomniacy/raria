#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-ed2k
//!
//! Native ED2K/eMule backend boundary for raria.
//!
//! This crate owns the future Rust implementation of ED2K/eMule protocol
//! behavior. The current checkpoint intentionally exposes only module
//! ownership boundaries and no runtime network behavior.

/// ED2K disk integrity and resume ownership.
pub mod disk;
/// AICH and ED2K hash ownership.
pub mod hash;
/// Stable ED2K client identity ownership.
pub mod identity;
/// Kad routing and search ownership.
pub mod kad;
/// ED2K link parsing ownership.
pub mod link;
/// Retained ED2K, eMule, and Kad opcode names.
pub mod opcode;
/// ED2K, eMule, and Kad packet framing.
pub mod packet;
/// ED2K peer session ownership.
pub mod peer;
/// ED2K protocol persistence ownership.
pub mod persist;
/// Server TCP and UDP ownership.
pub mod server;
/// Shared-file and upload cooperation ownership.
pub mod sharing;
/// ED2K source exchange and lifecycle ownership.
pub mod source;
/// ED2K typed tag codec.
pub mod tag;
/// Transfer planning and part payload ownership.
pub mod transfer;

mod wire;
