#![warn(missing_docs)]
//! # raria-core
//!
//! Core engine for the raria multi-protocol download manager.
//!
//! This crate provides the foundational types and orchestration logic:
//! - [`job::Job`] — download task model with state machine
//!   (Waiting → Active → Complete / Error / Paused / Removed)
//! - [`engine::Engine`] — central coordinator for job lifecycle
//! - [`scheduler::Scheduler`] — FIFO queue with configurable concurrency
//! - [`registry::JobRegistry`] — thread-safe in-memory job index
//! - [`cancel::CancelRegistry`] — per-job cancellation token management
//! - [`persist::Store`] — crash-safe persistence layer via `redb`
//! - [`progress::EventBus`] — `tokio::broadcast` channel for download events
//! - [`limiter`] — shared rate limiter using `governor` + `arc-swap`
//!
//! ## Architecture
//!
//! ```text
//! Engine ──┬── JobRegistry  (in-memory job index)
//!          ├── Scheduler    (waiting queue + concurrency limit)
//!          ├── CancelRegistry (per-job CancellationToken)
//!          ├── Store        (redb atomic persistence)
//!          └── EventBus     (progress / status broadcast)
//! ```
//!
//! Protocol-specific backends (HTTP, FTP, SFTP, BT) live in separate crates
//! and depend on `raria-core` for job model and engine types.

/// Per-job cancellation token management.
pub mod cancel;
/// Checksum verification (SHA-256, SHA-1, MD5).
pub mod checksum;
/// Global configuration struct and defaults.
pub mod config;
/// Configuration file parser (aria2-format key=value).
pub mod config_file;
/// Central download engine — job lifecycle coordinator.
pub mod engine;
/// Pre-allocation strategies for download files (fallocate / trunc / none).
pub mod file_alloc;
/// Input file parser (newline-delimited URI lists).
pub mod input_file;
/// Download job model: GID, status machine, options, metadata.
pub mod job;
/// Global and per-job rate limiting.
pub mod limiter;
/// Crash-safe persistence via redb (job state + segment checkpoints).
pub mod persist;
/// Broadcast event bus for download progress and status changes.
pub mod progress;
/// Thread-safe in-memory job index.
pub mod registry;
/// Output file rename strategies (append counter, overwrite, skip).
pub mod rename;
/// FIFO waiting-queue with configurable concurrency limit.
pub mod scheduler;
/// Byte-range segment planning and state tracking.
pub mod segment;
/// Protocol-agnostic download service trait.
pub mod service;
/// Per-job speed measurement over sliding windows.
pub mod speed;
