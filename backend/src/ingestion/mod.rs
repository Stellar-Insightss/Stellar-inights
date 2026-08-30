//! Ledger ingestion: pulls ledger data from Horizon/RPC and writes derived
//! corridor/anchor/network rows into a database in a way that survives
//! being re-run over the same range twice, and survives a reorg.
//!
//! The checkpoint carries `(ledger_sequence, ledger_hash)`, not sequence
//! alone, so a reorg is detectable directly — the next fetched ledger's
//! `prev_hash` stops matching the checkpoint's hash — rather than inferred
//! indirectly from a gap or a downstream inconsistency. Every derived write
//! is a full replace keyed on `(ledger_sequence, entity_id)`
//! ([`upsert::DerivedStore`]), so re-ingesting a range, or rolling one back
//! after a reorg and re-ingesting it, converges on the same stored state
//! instead of accumulating duplicates or leaving orphaned rows from an
//! abandoned fork.

pub mod fetch;
pub mod reconcile;
pub mod upsert;
pub mod watermark;

use std::time::Duration;

pub use fetch::{FakeLedgerSource, FetchedLedger, HorizonLedgerSource, LedgerSource};
pub use reconcile::{DriftAlert, DriftAlertSink, InMemoryDriftAlertSink, ReconciliationCheck};
pub use upsert::{DerivedStore, EntityId, InMemoryDerivedStore};
pub use watermark::{InMemoryWatermarkStore, Watermark, WatermarkStore};

#[derive(Debug, thiserror::Error)]
pub enum IngestionError {
    #[error("failed to fetch ledger {sequence}: {message}")]
    Fetch { sequence: u64, message: String },
    #[error("watermark store failure: {0}")]
    Watermark(String),
    #[error("derived store failure: {0}")]
    DerivedStore(String),
    #[error("alert sink failure: {0}")]
    Alert(String),
    #[error("reorg rollback target ledger {sequence} is unavailable")]
    ReorgRollbackTargetUnavailable { sequence: u64 },
}

/// Minimal, dependency-free FNV-1a hash, used where a deterministic,
/// cross-call-stable digest is needed but no cryptographic property is
/// required. `std`'s `DefaultHasher` is explicitly documented as not
/// suitable for this: its algorithm is unspecified and may change between
/// compilations, which would make [`FakeLedgerSource`]'s hash chain and
/// the reconciliation checksum unreliable to reason about.
pub(crate) fn fnv1a_u64(input: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub(crate) fn fnv1a_hex(input: &str) -> String {
    format!("{:016x}", fnv1a_u64(input))
}

/// The result of one [`IngestionPipeline::run_once`] call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    /// The next ledger has not closed yet; nothing to do this tick.
    Idle,
    /// One ledger was fetched and its rows upserted.
    Ingested { sequence: u64 },
    /// A reorg was detected and handled: the watermark was rolled back to
    /// `rolled_back_to` (or ingestion reset to start from scratch, see
    /// [`IngestionPipeline::start_sequence`]) and every derived row from
    /// `resumed_from` onward was invalidated. The caller's next `run_once`
    /// resumes forward ingestion from `resumed_from`.
    ReorgHandled {
        rolled_back_to: u64,
        resumed_from: u64,
    },
}

/// Orchestrates checkpointed, reorg-aware ledger ingestion.
pub struct IngestionPipeline<L, W, D> {
    source: L,
    watermark: W,
    derived: D,
    /// First ledger sequence this pipeline ever ingests.
    start_sequence: u64,
    /// How many ledgers back to roll on a detected reorg, on the assumption
    /// that a ledger this far behind the previous tip is now stable. This
    /// is a real assumption, not a proof — a reorg deeper than this will be
    /// only partially corrected — but it matches how block-explorer-style
    /// indexers commonly bound reorg handling in practice.
    confirmation_depth: u64,
}

impl<L, W, D> IngestionPipeline<L, W, D>
where
    L: LedgerSource,
    W: WatermarkStore,
    D: DerivedStore,
{
    pub fn new(
        source: L,
        watermark: W,
        derived: D,
        start_sequence: u64,
        confirmation_depth: u64,
    ) -> Self {
        Self {
            source,
            watermark,
            derived,
            start_sequence: start_sequence.max(1),
            confirmation_depth: confirmation_depth.max(1),
        }
    }

    /// The ledger source this pipeline reads from.
    pub fn source(&self) -> &L {
        &self.source
    }

    /// The checkpoint store, so a caller can inspect ingestion progress or
    /// share it with a [`reconcile::ReconciliationCheck`] over the same
    /// pipeline.
    pub fn watermark_store(&self) -> &W {
        &self.watermark
    }

    /// The derived-row store, so a caller can actually read what has been
    /// ingested (e.g. to serve it from an API).
    pub fn derived_store(&self) -> &D {
        &self.derived
    }

    /// Advances ingestion by at most one ledger.
    pub async fn run_once(&self) -> Result<IngestOutcome, IngestionError> {
        let current = self.watermark.load().await?;
        let next_sequence = match &current {
            Some(watermark) => watermark.sequence + 1,
            None => self.start_sequence,
        };

        let Some(fetched) = self.source.fetch_ledger(next_sequence).await? else {
            return Ok(IngestOutcome::Idle);
        };

        if let Some(watermark) = &current {
            if fetched.prev_hash != watermark.hash {
                return self.handle_reorg(watermark.clone()).await;
            }
        }

        self.derived.upsert(fetched.sequence, fetched.rows).await?;
        self.watermark
            .save(Watermark {
                sequence: fetched.sequence,
                hash: fetched.hash,
            })
            .await?;

        Ok(IngestOutcome::Ingested {
            sequence: fetched.sequence,
        })
    }

    async fn handle_reorg(&self, stale: Watermark) -> Result<IngestOutcome, IngestionError> {
        let floor = self.start_sequence.saturating_sub(1);
        let rollback_target = stale
            .sequence
            .saturating_sub(self.confirmation_depth)
            .max(floor);

        if rollback_target <= floor {
            self.derived.invalidate_from(self.start_sequence).await?;
            self.watermark.clear().await?;
            return Ok(IngestOutcome::ReorgHandled {
                rolled_back_to: floor,
                resumed_from: self.start_sequence,
            });
        }

        let anchor = self.source.fetch_ledger(rollback_target).await?.ok_or(
            IngestionError::ReorgRollbackTargetUnavailable {
                sequence: rollback_target,
            },
        )?;

        self.derived.invalidate_from(rollback_target + 1).await?;
        self.watermark
            .save(Watermark {
                sequence: anchor.sequence,
                hash: anchor.hash,
            })
            .await?;

        Ok(IngestOutcome::ReorgHandled {
            rolled_back_to: rollback_target,
            resumed_from: rollback_target + 1,
        })
    }

    /// Runs [`Self::run_once`] on a fixed interval, forever. A failed tick
    /// is logged and retried next interval rather than ending the loop —
    /// unlike a `?`-propagating `loop`, one transient fetch failure does
    /// not permanently kill background ingestion.
    pub async fn run_forever(&self, poll_interval: Duration) {
        let mut ticker = tokio::time::interval(poll_interval);
        loop {
            ticker.tick().await;
            match self.run_once().await {
                Ok(IngestOutcome::Idle) => {}
                Ok(outcome) => tracing::debug!(?outcome, "ingestion tick"),
                Err(error) => {
                    tracing::warn!(error = %error, "ingestion tick failed; retrying next tick");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::generator::RawSnapshotRow;

    fn row(sequence: u64, corridor: &str) -> RawSnapshotRow {
        RawSnapshotRow {
            ledger_sequence: sequence,
            corridor: corridor.to_string(),
            source: "testnet".to_string(),
            reliability: 1.0,
            volume: 1.0,
            latency_ms: 1.0,
        }
    }

    fn pipeline(
        source: FakeLedgerSource,
    ) -> IngestionPipeline<FakeLedgerSource, InMemoryWatermarkStore, InMemoryDerivedStore> {
        IngestionPipeline::new(
            source,
            InMemoryWatermarkStore::new(),
            InMemoryDerivedStore::new(),
            1,
            3,
        )
    }

    #[tokio::test]
    async fn ingests_ledgers_one_at_a_time_and_advances_the_watermark() {
        let source = FakeLedgerSource::new();
        source.append("a", vec![row(1, "eur/usd")]).await;
        source.append("a", vec![row(2, "eur/usd")]).await;

        let pipeline = pipeline(source);

        assert_eq!(
            pipeline.run_once().await.unwrap(),
            IngestOutcome::Ingested { sequence: 1 }
        );
        assert_eq!(
            pipeline.run_once().await.unwrap(),
            IngestOutcome::Ingested { sequence: 2 }
        );
        assert_eq!(pipeline.run_once().await.unwrap(), IngestOutcome::Idle);

        let rows = pipeline.derived_store().rows_in_range(1, 2).await.unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn re_running_over_an_already_processed_range_is_byte_identical() {
        let source = FakeLedgerSource::new();
        source.append("a", vec![row(1, "eur/usd")]).await;
        source.append("a", vec![row(2, "eur/usd")]).await;

        let pipeline = pipeline(source);
        pipeline.run_once().await.unwrap();
        pipeline.run_once().await.unwrap();
        let first_pass = pipeline.derived_store().rows_in_range(1, 2).await.unwrap();

        // Force a full re-ingest of the same range without going through a
        // reorg: reset the watermark and re-run from the start.
        pipeline.watermark_store().clear().await.unwrap();
        pipeline.run_once().await.unwrap();
        pipeline.run_once().await.unwrap();
        let second_pass = pipeline.derived_store().rows_in_range(1, 2).await.unwrap();

        assert_eq!(
            first_pass, second_pass,
            "aggregate rows must be byte-identical on re-run"
        );
    }

    #[tokio::test]
    async fn a_reorg_rolls_back_by_the_confirmation_depth_and_drops_orphaned_rows() {
        let source = FakeLedgerSource::new();
        for seq in 1..=5u64 {
            source.append("a", vec![row(seq, "eur/usd")]).await;
        }

        let pipeline = pipeline(source);
        for _ in 1..=5 {
            pipeline.run_once().await.unwrap();
        }
        assert_eq!(
            pipeline
                .derived_store()
                .rows_in_range(1, 5)
                .await
                .unwrap()
                .len(),
            5
        );

        // Ledgers 4 and 5 get reorged out onto a new fork that also extends
        // one ledger past the old tip — a reorg is only observable once a
        // new ledger with different ancestry actually arrives, so the
        // replacement must reach ledger 6 for the pipeline to notice
        // anything happened. The new fork carries a different corridor so a
        // leftover orphan from the old fork is unmistakable.
        pipeline
            .source()
            .fork_from(
                4,
                vec![
                    ("b", vec![row(4, "gbp/usd")]),
                    ("b", vec![row(5, "gbp/usd")]),
                    ("b", vec![row(6, "gbp/usd")]),
                ],
            )
            .await;

        let outcome = pipeline.run_once().await.unwrap();
        assert_eq!(
            outcome,
            IngestOutcome::ReorgHandled {
                rolled_back_to: 2,
                resumed_from: 3
            }
        );

        // Walk forward again to fully re-ingest the corrected chain.
        loop {
            match pipeline.run_once().await.unwrap() {
                IngestOutcome::Idle => break,
                _ => continue,
            }
        }

        let final_rows = pipeline.derived_store().rows_in_range(1, 6).await.unwrap();
        let corridors: std::collections::BTreeSet<_> =
            final_rows.iter().map(|r| r.corridor.as_str()).collect();

        assert_eq!(final_rows.len(), 6, "no duplicate or orphaned rows");
        assert!(
            corridors.contains("gbp/usd"),
            "new fork's rows must be present"
        );
        assert!(
            !final_rows
                .iter()
                .any(|r| r.ledger_sequence >= 4 && r.corridor == "eur/usd"),
            "old fork's rows at reorged sequences must not survive"
        );
    }

    #[tokio::test]
    async fn a_reorg_deeper_than_the_start_sequence_resets_ingestion_entirely() {
        let source = FakeLedgerSource::new();
        source.append("a", vec![row(1, "eur/usd")]).await;
        source.append("a", vec![row(2, "eur/usd")]).await;

        let pipeline = IngestionPipeline::new(
            source,
            InMemoryWatermarkStore::new(),
            InMemoryDerivedStore::new(),
            1,
            10, // confirmation depth far exceeds the chain ingested so far
        );
        pipeline.run_once().await.unwrap();
        pipeline.run_once().await.unwrap();

        // Extend one ledger past the old tip (2), so the reorg is actually
        // observable — see the comment in the test above.
        pipeline
            .source()
            .fork_from(
                2,
                vec![
                    ("b", vec![row(2, "gbp/usd")]),
                    ("b", vec![row(3, "gbp/usd")]),
                ],
            )
            .await;

        let outcome = pipeline.run_once().await.unwrap();
        assert_eq!(
            outcome,
            IngestOutcome::ReorgHandled {
                rolled_back_to: 0,
                resumed_from: 1
            }
        );
        assert_eq!(pipeline.watermark_store().load().await.unwrap(), None);
        assert!(pipeline
            .derived_store()
            .rows_in_range(0, 100)
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn fnv1a_is_deterministic_and_sensitive_to_input() {
        assert_eq!(fnv1a_u64("a"), fnv1a_u64("a"));
        assert_ne!(fnv1a_u64("a"), fnv1a_u64("b"));
        assert_eq!(fnv1a_hex("a").len(), 16);
    }
}
