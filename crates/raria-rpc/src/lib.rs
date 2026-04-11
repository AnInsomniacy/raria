#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-rpc
//!
//! aria2-compatible JSON-RPC/WebSocket server for raria.
//!
//! Provides the full aria2 JSON-RPC interface including `system.multicall`,
//! token-based authentication, WebSocket push notifications, and CORS.
//!
//! ## Modules
//!
//! - [`methods`] — RPC method implementations (add, pause, remove, tellStatus…)
//! - [`server`] — HTTP + WebSocket transport, auth wrapping, CORS
//! - [`facade`] — conversion between raria-core types and aria2 response format
//! - [`events`] — mapping download events to aria2 notification methods

/// Download event to aria2 notification mapping.
pub mod events;
/// Conversion between raria-core types and aria2 JSON response format.
pub mod facade;
/// RPC method implementations (aria2 JSON-RPC parity).
pub mod methods;
/// HTTP + WebSocket server, authentication, and CORS.
pub mod server;
