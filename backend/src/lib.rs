//! Stellar Insights Backend
//!
//! Core backend services for the Stellar Insights platform, including:
//! - Real-time data processing and fan-out
//! - Distributed locking for safe concurrent job execution
//! - WebSocket connection management

pub mod contract_ops;
pub mod distributed_lock;
pub mod event_indexer;
pub mod observability;
pub mod realtime;
