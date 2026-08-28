//! Schema-versioned ingestion for Soroban contract events.
//!
//! [`EventIndexer`] accepts protocol-native XDR events, dispatches them to an
//! exact schema parser, and writes the normalized result through
//! [`AnalyticsSink`]. Production storage or queue adapters implement that
//! trait; [`InMemoryAnalyticsSink`] provides deterministic local/test storage.

pub mod dispatch;
mod indexer;
pub mod parsers;
mod sink;
mod xdr;

pub use dispatch::{
    dispatch, DispatchError, NormalizedContractEvent, NormalizedEvent, NormalizedSnapshotSubmitted,
};
pub use indexer::{EventIndexer, IndexerError};
pub use sink::{AnalyticsSink, AnalyticsSinkError, InMemoryAnalyticsSink};
