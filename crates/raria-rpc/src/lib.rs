#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-rpc
//!
//! Native HTTP JSON API and WebSocket event stream for raria.
//!
//! The retained public contract is `/api/v1` resources plus
//! `/api/v1/events`. The JSON-RPC modules remain temporary implementation
//! debt until native coverage is sufficient to delete them.
//!
//! ## Modules
//!
//! - [`api`] — native HTTP resources and event stream
//! - [`methods`] — temporary JSON-RPC implementation pending deletion
//! - [`server`] — temporary shared listener pending native-only replacement
//! - [`facade`] — temporary JSON-RPC projection pending deletion
//! - [`events`] — temporary legacy notification projection pending deletion

mod metalink_tasks;

/// Native raria HTTP JSON API.
pub mod api;
/// Download event to aria2 notification mapping.
pub mod events;
/// Conversion between raria-core types and aria2 JSON response format.
pub mod facade;
/// RPC method implementations for the declared aria2-style surface.
pub mod methods;
/// HTTP + WebSocket server, authentication, and CORS.
pub mod server;
