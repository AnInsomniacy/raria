#![deny(unsafe_code)]
#![warn(missing_docs)]
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

/// Conversion of parsed Metalink data to raria download jobs.
pub mod normalizer;
/// XML deserialization of Metalink 4.0 documents.
pub mod parser;
