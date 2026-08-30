use serde::{Deserialize, Serialize};

pub const DETERMINISM_EFFECTIVE_DATE: &str = "2025-01-01";
pub const PINNED_MODEL_VERSION: &str = "reliability-v2025.01";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelVersion {
    pub name: String,
    pub effective_from: String,
    pub effective_until: Option<String>,
}

/// The historical replay path is pinned to a single model version so that
/// replaying the same ledger range produces identical output regardless of the
/// model currently shipped in production.
///
/// This guarantee applies to data on or after 2025-01-01, which is the
/// effective date of the snapshot determinism contract. Model changes after that
/// date are not retroactively applied to historical replays.
pub fn model_version_for_range(start_ledger: u64, end_ledger: u64) -> &'static str {
    let _ = (start_ledger, end_ledger);
    PINNED_MODEL_VERSION
}

pub fn model_version_for_ledger_range(start_ledger: u64, end_ledger: u64) -> &'static str {
    model_version_for_range(start_ledger, end_ledger)
}

pub fn network_model_version(network: &str) -> &'static str {
    let _ = network;
    PINNED_MODEL_VERSION
}
