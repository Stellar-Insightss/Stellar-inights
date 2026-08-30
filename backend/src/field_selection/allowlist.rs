//! Field allowlists per endpoint.
//!
//! Each endpoint/struct that supports field selection must have an explicit allowlist
//! defined here. The allowlist maps client-provided field names to their SQL/output representation.
//!
//! **Design principle**: The allowlist is a deliberate subset of the struct's fields.
//! Adding a new field to the Rust struct does NOT automatically expose it via the API.

use std::collections::HashMap;

/// Retrieves the field allowlist for a given endpoint.
///
/// # Arguments
///
/// * `endpoint` - The endpoint identifier (e.g., "snapshots", "aggregates")
///
/// # Returns
///
/// A reference to the allowlist `HashMap`, or `None` if no allowlist is defined for this endpoint.
pub fn get_allowlist(endpoint: &str) -> Option<&'static HashMap<&'static str, &'static str>> {
    match endpoint {
        "snapshots" => Some(&SNAPSHOTS_ALLOWLIST),
        "aggregates" => Some(&AGGREGATES_ALLOWLIST),
        _ => None,
    }
}

// ─── SNAPSHOTS ENDPOINT ─────────────────────────────────────────────────────────
//
// Based on: backend/src/event_indexer/dispatch.rs::NormalizedSnapshotSubmitted
//
// Real struct fields:
//   pub epoch: u64,
//   pub snapshot_hash: String,
//   pub source_data_hash: String,  <- DELIBERATELY EXCLUDED
//   pub submitted_at: u64,
//   pub submitter: String,         <- DELIBERATELY EXCLUDED (privacy)
//
// Allowlist rationale:
// - `epoch` and `submitted_at` are audit metadata, safe to expose.
// - `snapshot_hash` is the canonical identifier, needed by clients.
// - `source_data_hash` is internal reconciliation state; clients don't need it.
// - `submitter` could reveal private operator identity; excluded for privacy.
//
// When this endpoint becomes HTTP, the handler will:
//   1. Call `parse_fields(request_query_param, "snapshots")?`
//   2. Receive `Vec<&'static str>` with validated field names (e.g., ["epoch", "snapshot_hash"])
//   3. Build the response projection using only those fields
//
lazy_static::lazy_static! {
    static ref SNAPSHOTS_ALLOWLIST: HashMap<&'static str, &'static str> = {
        [
            ("epoch", "epoch"),
            ("snapshot_hash", "snapshot_hash"),
            ("submitted_at", "submitted_at"),
        ]
        .iter()
        .copied()
        .collect()
    };
}

// ─── AGGREGATES ENDPOINT ────────────────────────────────────────────────────────
//
// Based on: backend/src/reconciliation/spec.rs::OffChainAggregate
//
// Real struct fields:
//   pub period: u64,
//   pub snapshot_hash: [u8; 32],
//   pub source_data_hash: [u8; 32],  <- DELIBERATELY EXCLUDED
//
// Allowlist rationale:
// - `period` identifies the reconciliation epoch, needed by clients.
// - `snapshot_hash` is the canonical proof, needed by clients.
// - `source_data_hash` is internal implementation detail; clients request snapshots
//   by comparing hashes, not raw source data.
//
// When this endpoint becomes HTTP, the handler will:
//   1. Call `parse_fields(request_query_param, "aggregates")?`
//   2. Receive `Vec<&'static str>` with validated field names
//   3. Serialize only selected fields to JSON
//
lazy_static::lazy_static! {
    static ref AGGREGATES_ALLOWLIST: HashMap<&'static str, &'static str> = {
        [
            ("period", "period"),
            ("snapshot_hash", "snapshot_hash"),
        ]
        .iter()
        .copied()
        .collect()
    };
}

// ─── REGISTRY & EXTENSION POINTS ───────────────────────────────────────────────
//
// For more complex scenarios (e.g., dynamic field mapping, per-tenant allowlists),
// consider implementing an `AllowlistRegistry` trait:
//
//     pub trait AllowlistRegistry {
//         fn get_allowlist(&self, endpoint: &str) -> Option<&'static HashMap<&'static str, &'static str>>;
//     }
//
// This is not implemented by default because the current design favors explicitness:
// each endpoint's allowlist is a top-level definition, not a plugin.

/// Marker trait for future dynamic allowlist registration.
/// Currently unused; provided for documentation and future expansion.
pub trait AllowlistRegistry {
    /// Retrieve an allowlist for the given endpoint.
    fn get_allowlist(&self, endpoint: &str) -> Option<&'static HashMap<&'static str, &'static str>>;
}

/// A simple in-memory registry that delegates to `get_allowlist()`.
pub struct StaticRegistry;

impl AllowlistRegistry for StaticRegistry {
    fn get_allowlist(&self, endpoint: &str) -> Option<&'static HashMap<&'static str, &'static str>> {
        get_allowlist(endpoint)
    }
}
