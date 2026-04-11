#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-sftp
//!
//! SFTP download backend for raria.
//!
//! Implements [`raria_range::backend::ByteSourceBackend`] using `russh`
//! (pure-Rust SSH), with support for password / key-based authentication,
//! host key verification, and byte-range reads.

/// SFTP backend implementing `ByteSourceBackend`.
pub mod backend;
