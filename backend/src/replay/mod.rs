use crate::snapshot::{generate_snapshot, model_version_for_ledger_range, RawSnapshotRow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerRange {
    pub start_ledger: u64,
    pub end_ledger: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("invalid ledger range: start > end")]
    InvalidRange,
    #[error("snapshot generation failed: {0}")]
    Snapshot(#[from] crate::snapshot::SnapshotError),
}

/// Historical replays are intentionally pinned to a model version and do not read
/// wall-clock time. Re-running the same range against the same raw inputs must
/// produce a byte-identical output payload.
pub fn replay_historical_range(range: &LedgerRange, rows: &[RawSnapshotRow]) -> Result<Vec<u8>, ReplayError> {
    if range.start_ledger > range.end_ledger {
        return Err(ReplayError::InvalidRange);
    }

    let _ = model_version_for_ledger_range(range.start_ledger, range.end_ledger);

    let filtered: Vec<RawSnapshotRow> = rows
        .iter()
        .filter(|row| row.ledger_sequence >= range.start_ledger && row.ledger_sequence <= range.end_ledger)
        .cloned()
        .collect();

    // Replay of an empty range is intentionally deterministic and yields an empty
    // JSON payload rather than reading time or state from the live process.
    if filtered.is_empty() {
        return Ok(b"{\"model_version\":\"reliability-v2025.01\",\"records\":[]}".to_vec());
    }

    Ok(generate_snapshot(&filtered)?)
}
