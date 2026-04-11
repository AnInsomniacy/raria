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

pub mod cancel;
pub mod checksum;
pub mod config;
pub mod config_file;
pub mod engine;
pub mod file_alloc;
pub mod input_file;
pub mod job;
pub mod limiter;
pub mod persist;
pub mod progress;
pub mod registry;
pub mod rename;
pub mod scheduler;
pub mod segment;
pub mod service;
pub mod speed;
