use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawSnapshotRow {
    pub ledger_sequence: u64,
    pub corridor: String,
    pub source: String,
    pub reliability: f64,
    pub volume: f64,
    pub latency_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub corridor: String,
    pub source: String,
    pub avg_reliability: f64,
    pub total_volume: f64,
    pub avg_latency_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPayload {
    pub model_version: String,
    pub records: Vec<SnapshotRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot input is empty")]
    EmptyInput,
    #[error("snapshot serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Pure, deterministic snapshot generation.
///
/// The function aggregates by corridor using a canonical BTreeMap ordering and
/// sorts ledger rows before computing averages so the serialized output is
/// stable across repeated replays of the same historical data.
pub fn generate_snapshot(rows: &[RawSnapshotRow]) -> Result<Vec<u8>, SnapshotError> {
    if rows.is_empty() {
        return Err(SnapshotError::EmptyInput);
    }

    let mut grouped: BTreeMap<String, Vec<&RawSnapshotRow>> = BTreeMap::new();
    for row in rows {
        grouped.entry(row.corridor.clone()).or_default().push(row);
    }

    let mut records = Vec::with_capacity(grouped.len());
    for (corridor, entries) in grouped {
        let mut entries = entries;
        entries.sort_by_key(|entry| entry.ledger_sequence);

        let total_count = entries.len() as f64;
        let avg_reliability = entries.iter().map(|entry| entry.reliability).sum::<f64>() / total_count;
        let total_volume = entries.iter().map(|entry| entry.volume).sum::<f64>();
        let avg_latency_ms = entries.iter().map(|entry| entry.latency_ms).sum::<f64>() / total_count;

        records.push(SnapshotRecord {
            corridor,
            source: entries[0].source.clone(),
            avg_reliability: avg_reliability,
            total_volume,
            avg_latency_ms,
        });
    }

    let payload = SnapshotPayload {
        model_version: crate::snapshot::model_version::PINNED_MODEL_VERSION.to_string(),
        records,
    };

    Ok(serde_json::to_vec(&payload)?)
}
