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

pub mod backend;
pub mod content_disposition;
pub mod cookies;
