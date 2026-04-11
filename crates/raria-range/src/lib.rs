#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # raria-range
//!
//! Protocol-agnostic byte-range download executor.
//!
//! Provides the [`backend::ByteSourceBackend`] trait that all download
//! backends (HTTP, FTP, SFTP) must implement, and the
//! [`executor::SegmentExecutor`] that orchestrates parallel segment
//! downloads with progress tracking, checkpointing, and error retry.
//!
//! ## Key Types
//!
//! - [`backend::ByteSourceBackend`] — trait for fetching byte ranges
//! - [`executor::SegmentExecutor`] — parallel segment download orchestrator

/// Protocol-agnostic download backend trait and shared types.
pub mod backend;
/// Parallel segment download executor with retry and checkpointing.
pub mod executor;
