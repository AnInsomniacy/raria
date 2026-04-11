#![deny(unsafe_code)]
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

pub mod backend;
pub mod executor;
