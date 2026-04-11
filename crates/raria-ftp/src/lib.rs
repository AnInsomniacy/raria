#![warn(missing_docs)]
//! # raria-ftp
//!
//! FTP/FTPS download backend for raria.
//!
//! Implements [`raria_range::backend::ByteSourceBackend`] using `suppaftp`,
//! with support for passive mode, explicit FTPS, and byte-range REST/RETR.

/// FTP/FTPS backend implementing `ByteSourceBackend`.
pub mod backend;
