//! Idempotent, ledger-keyed storage for derived rows.
//!
//! Every write is a full replace of the row set for one `(ledger_sequence,
//! entity_id)` pair, never a blind insert. Re-ingesting the same ledger with
//! the same inputs therefore always converges on the same stored value
//! instead of accumulating duplicates, and rolling a reorg back is just
//! deleting every key at or after the rollback point.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::snapshot::generator::RawSnapshotRow;

use super::IngestionError;

/// Identifies one derived row within a ledger: which network it came from,
/// and which corridor it describes. Two rows with the same `(ledger_sequence,
/// EntityId)` are the same logical fact and the second write replaces the
/// first — they never coexist.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityId {
    pub source: String,
    pub corridor: String,
}

impl EntityId {
    fn of(row: &RawSnapshotRow) -> Self {
        Self {
            source: row.source.clone(),
            corridor: row.corridor.clone(),
        }
    }
}

/// Idempotent, ledger-sequence-keyed storage for [`RawSnapshotRow`]s.
#[async_trait]
pub trait DerivedStore: Send + Sync {
    /// Replaces the stored rows for `sequence` with `rows`. Any entity
    /// previously stored for this sequence but absent from `rows` is
    /// dropped — the write is a full replace of that ledger's rows, not a
    /// merge, so a corridor that no longer applies (e.g. after a reorg
    /// replaced it) cannot linger as a stale orphan.
    async fn upsert(&self, sequence: u64, rows: Vec<RawSnapshotRow>) -> Result<(), IngestionError>;

    /// Deletes every stored row with `ledger_sequence >= from_sequence`.
    /// Used to unwind the abandoned side of a reorg before re-ingesting the
    /// canonical chain over the same range.
    async fn invalidate_from(&self, from_sequence: u64) -> Result<(), IngestionError>;

    /// Returns every stored row with `start <= ledger_sequence <= end`,
    /// recomputed fresh from what is actually stored — never from an
    /// incrementally-maintained running total, so a rolled-back reorg can
    /// never leave a stale aggregate behind.
    async fn rows_in_range(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<RawSnapshotRow>, IngestionError>;
}

/// Reference [`DerivedStore`] backed by process memory.
#[derive(Debug, Default)]
pub struct InMemoryDerivedStore {
    rows: tokio::sync::Mutex<BTreeMap<(u64, EntityId), RawSnapshotRow>>,
}

impl InMemoryDerivedStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DerivedStore for InMemoryDerivedStore {
    async fn upsert(&self, sequence: u64, rows: Vec<RawSnapshotRow>) -> Result<(), IngestionError> {
        let mut guard = self.rows.lock().await;

        guard.retain(|(seq, _), _| *seq != sequence);
        for row in rows {
            guard.insert((sequence, EntityId::of(&row)), row);
        }

        Ok(())
    }

    async fn invalidate_from(&self, from_sequence: u64) -> Result<(), IngestionError> {
        self.rows
            .lock()
            .await
            .retain(|(seq, _), _| *seq < from_sequence);
        Ok(())
    }

    async fn rows_in_range(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<RawSnapshotRow>, IngestionError> {
        Ok(self
            .rows
            .lock()
            .await
            .iter()
            .filter(|((seq, _), _)| *seq >= start && *seq <= end)
            .map(|(_, row)| row.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(sequence: u64, corridor: &str, volume: f64) -> RawSnapshotRow {
        RawSnapshotRow {
            ledger_sequence: sequence,
            corridor: corridor.to_string(),
            source: "testnet".to_string(),
            reliability: 1.0,
            volume,
            latency_ms: 10.0,
        }
    }

    #[tokio::test]
    async fn re_upserting_the_same_ledger_does_not_duplicate_rows() {
        let store = InMemoryDerivedStore::new();

        store
            .upsert(10, vec![row(10, "eur/usd", 5.0)])
            .await
            .unwrap();
        store
            .upsert(10, vec![row(10, "eur/usd", 5.0)])
            .await
            .unwrap();

        let rows = store.rows_in_range(10, 10).await.unwrap();
        assert_eq!(rows.len(), 1, "re-upsert must replace, not append");
        assert_eq!(rows[0].volume, 5.0);
    }

    #[tokio::test]
    async fn re_upserting_with_a_different_value_replaces_it() {
        let store = InMemoryDerivedStore::new();

        store
            .upsert(10, vec![row(10, "eur/usd", 5.0)])
            .await
            .unwrap();
        store
            .upsert(10, vec![row(10, "eur/usd", 9.0)])
            .await
            .unwrap();

        let rows = store.rows_in_range(10, 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].volume, 9.0);
    }

    #[tokio::test]
    async fn upsert_drops_entities_no_longer_present_for_that_ledger() {
        let store = InMemoryDerivedStore::new();

        store
            .upsert(10, vec![row(10, "eur/usd", 5.0), row(10, "btc/usd", 1.0)])
            .await
            .unwrap();
        // Second upsert for the same sequence only carries eur/usd — this is
        // exactly what a reorg replacement looks like when the new fork's
        // ledger doesn't reproduce every corridor the old fork had.
        store
            .upsert(10, vec![row(10, "eur/usd", 5.0)])
            .await
            .unwrap();

        let rows = store.rows_in_range(10, 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].corridor, "eur/usd");
    }

    #[tokio::test]
    async fn invalidate_from_removes_only_the_targeted_range() {
        let store = InMemoryDerivedStore::new();
        store.upsert(1, vec![row(1, "eur/usd", 1.0)]).await.unwrap();
        store.upsert(2, vec![row(2, "eur/usd", 2.0)]).await.unwrap();
        store.upsert(3, vec![row(3, "eur/usd", 3.0)]).await.unwrap();

        store.invalidate_from(2).await.unwrap();

        let remaining = store.rows_in_range(0, 100).await.unwrap();
        let sequences: Vec<u64> = remaining.iter().map(|r| r.ledger_sequence).collect();
        assert_eq!(sequences, vec![1]);
    }

    #[tokio::test]
    async fn rows_in_range_is_bounded_on_both_ends() {
        let store = InMemoryDerivedStore::new();
        for seq in 1..=5 {
            store
                .upsert(seq, vec![row(seq, "eur/usd", seq as f64)])
                .await
                .unwrap();
        }

        let rows = store.rows_in_range(2, 4).await.unwrap();
        let mut sequences: Vec<u64> = rows.iter().map(|r| r.ledger_sequence).collect();
        sequences.sort_unstable();
        assert_eq!(sequences, vec![2, 3, 4]);
    }
}
