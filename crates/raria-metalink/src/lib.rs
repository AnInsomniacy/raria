//! # raria-metalink
//!
//! Metalink 4.0 (RFC 5854) parser for raria.
//!
//! Parses `.metalink` / `.meta4` XML files and normalizes them into
//! raria-compatible multi-mirror download jobs.
//!
//! ## Modules
//!
//! - [`parser`] — XML deserialization of Metalink documents
//! - [`normalizer`] — conversion to raria's internal job representation

pub mod normalizer;
pub mod parser;
