#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-rpc
//!
//! Native HTTP JSON API and WebSocket event stream for raria.
//!
//! The retained public contract is `/api/v1` resources plus
//! `/api/v1/events`.
//!
//! ## Modules
//!
//! - [`api`] — native HTTP resources and event stream

mod metalink_tasks;

/// Native raria HTTP JSON API.
pub mod api;
