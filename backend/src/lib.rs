//! Stellar Insights Backend
//!
//! Core backend services for the Stellar Insights platform, including:
//! - Real-time data processing and fan-out
//! - Distributed locking for safe concurrent job execution
//! - WebSocket connection management
//! - Sparse fieldset selection with explicit allowlisting (ready for HTTP layer)

pub mod contract_ops;
pub mod distributed_lock;
pub mod event_indexer;
pub mod field_selection;
pub mod observability;
pub mod realtime;
pub mod reconciliation;
