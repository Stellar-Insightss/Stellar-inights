//! Stellar Insights Backend
//!
//! Core backend services for the Stellar Insights platform, including:
//! - Real-time data processing and fan-out
//! - Real-time payment reliability and latency percentile analytics
//! - Distributed locking for safe concurrent job execution
//! - WebSocket connection management

pub mod analytics;
pub mod contract_ops;
pub mod distributed_lock;
pub mod event_indexer;
pub mod network;
pub mod observability;
pub mod realtime;
pub mod reconciliation;
pub mod replay;
pub mod snapshot;
