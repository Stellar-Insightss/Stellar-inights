//! The ingestion checkpoint: the last ledger successfully ingested, keyed on
//! both its sequence number *and* its hash.
//!
//! A checkpoint keyed on sequence alone can only detect a reorg indirectly,
//! as a gap or a downstream inconsistency. Carrying the hash alongside the
//! sequence makes a reorg directly observable: the next fetched ledger's
//! `prev_hash` either matches this hash (linear continuation) or it doesn't
//! (the chain moved out from under us).

use async_trait::async_trait;

use super::IngestionError;

/// The last ledger this pipeline has durably committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watermark {
    pub sequence: u64,
    pub hash: String,
}

/// Durable storage for the ingestion checkpoint.
///
/// Implementors must make `save` and `load` consistent with a single logical
/// checkpoint: a `load` that races a `save` may return either the old or the
/// new value, but never a value that was never saved.
#[async_trait]
pub trait WatermarkStore: Send + Sync {
    async fn load(&self) -> Result<Option<Watermark>, IngestionError>;
    async fn save(&self, watermark: Watermark) -> Result<(), IngestionError>;
    /// Clears the checkpoint entirely, so the next `load` returns `None`.
    ///
    /// Used when a reorg's confirmation depth reaches back past the
    /// pipeline's configured start ledger — there is no stable ledger left
    /// to roll back to, so ingestion must restart from scratch.
    async fn clear(&self) -> Result<(), IngestionError>;
}

/// Reference [`WatermarkStore`] backed by process memory.
///
/// Not durable across restarts — a production deployment plugs in a real
/// persistent store behind the same trait, mirroring how
/// [`InMemoryAnalyticsSink`](crate::event_indexer::InMemoryAnalyticsSink) and
/// the reconciliation module's in-memory stores stand in for production
/// adapters elsewhere in this crate.
#[derive(Debug, Default)]
pub struct InMemoryWatermarkStore {
    state: tokio::sync::Mutex<Option<Watermark>>,
}

impl InMemoryWatermarkStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WatermarkStore for InMemoryWatermarkStore {
    async fn load(&self) -> Result<Option<Watermark>, IngestionError> {
        Ok(self.state.lock().await.clone())
    }

    async fn save(&self, watermark: Watermark) -> Result<(), IngestionError> {
        *self.state.lock().await = Some(watermark);
        Ok(())
    }

    async fn clear(&self) -> Result<(), IngestionError> {
        *self.state.lock().await = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starts_empty() {
        let store = InMemoryWatermarkStore::new();
        assert_eq!(store.load().await.unwrap(), None);
    }

    #[tokio::test]
    async fn round_trips_a_saved_watermark() {
        let store = InMemoryWatermarkStore::new();
        let watermark = Watermark {
            sequence: 42,
            hash: "abc123".to_string(),
        };

        store.save(watermark.clone()).await.unwrap();

        assert_eq!(store.load().await.unwrap(), Some(watermark));
    }

    #[tokio::test]
    async fn a_later_save_overwrites_the_earlier_one() {
        let store = InMemoryWatermarkStore::new();
        store
            .save(Watermark {
                sequence: 1,
                hash: "a".to_string(),
            })
            .await
            .unwrap();
        store
            .save(Watermark {
                sequence: 2,
                hash: "b".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(
            store.load().await.unwrap(),
            Some(Watermark {
                sequence: 2,
                hash: "b".to_string()
            })
        );
    }

    #[tokio::test]
    async fn clear_resets_to_empty() {
        let store = InMemoryWatermarkStore::new();
        store
            .save(Watermark {
                sequence: 7,
                hash: "x".to_string(),
            })
            .await
            .unwrap();

        store.clear().await.unwrap();

        assert_eq!(store.load().await.unwrap(), None);
    }
}
