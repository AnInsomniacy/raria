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

pub mod events;
pub mod facade;
pub mod methods;
pub mod server;
