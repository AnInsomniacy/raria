#![deny(unsafe_code)]
//! # raria-sftp
//!
//! SFTP download backend for raria.
//!
//! Implements [`raria_range::backend::ByteSourceBackend`] using `russh`
//! (pure-Rust SSH), with support for password / key-based authentication,
//! host key verification, and byte-range reads.

pub mod backend;
