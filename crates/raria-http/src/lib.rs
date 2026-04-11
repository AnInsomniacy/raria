#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-http
//!
//! HTTP/HTTPS download backend for raria.
//!
//! Implements [`raria_range::backend::ByteSourceBackend`] using `reqwest`,
//! with support for:
//! - Range requests for parallel segmented downloads
//! - Cookie persistence via [`cookies`]
//! - Content-Disposition header parsing via [`content_disposition`]
//! - Proxy support (HTTP, SOCKS5) and TLS (rustls)
//!
//! ## Modules
//!
//! - [`backend`] — `HttpBackend` implementing `ByteSourceBackend`
//! - [`content_disposition`] — filename extraction from response headers
//! - [`cookies`] — cookie jar loading / persistence

/// HTTP/HTTPS backend implementing `ByteSourceBackend`.
pub mod backend;
/// Content-Disposition header parser for filename extraction.
pub mod content_disposition;
/// Cookie jar persistence (Netscape format).
pub mod cookies;
