//! Scheduled drift detection: recomputes a checksum directly from raw
//! ledger data and compares it against what the pipeline actually has
//! stored, so a bug in the ingestion path itself — not just an outage —
//! gets caught.
//!
//! Watermark-based checkpointing proves the pipeline *ran*; it says nothing
//! about whether what it wrote still matches what the source of truth would
//! produce today. This is the independent check for that.

use std::time::Duration;

use async_trait::async_trait;

use crate::snapshot::generator::RawSnapshotRow;

use super::fetch::LedgerSource;
use super::upsert::DerivedStore;
use super::watermark::WatermarkStore;
use super::{fnv1a_u64, IngestionError};

/// A detected mismatch between raw and derived data over one window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftAlert {
    pub window_start: u64,
    pub window_end: u64,
    pub raw_checksum: u64,
    pub derived_checksum: u64,
}

#[async_trait]
pub trait DriftAlertSink: Send + Sync {
    async fn emit(&self, alert: DriftAlert) -> Result<(), IngestionError>;
}

/// Reference [`DriftAlertSink`] that just records what it was given.
#[derive(Debug, Default)]
pub struct InMemoryDriftAlertSink {
    alerts: tokio::sync::Mutex<Vec<DriftAlert>>,
}

impl InMemoryDriftAlertSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn alerts(&self) -> Vec<DriftAlert> {
        self.alerts.lock().await.clone()
    }
}

#[async_trait]
impl DriftAlertSink for InMemoryDriftAlertSink {
    async fn emit(&self, alert: DriftAlert) -> Result<(), IngestionError> {
        self.alerts.lock().await.push(alert);
        Ok(())
    }
}

/// Order-independent checksum over a set of rows.
///
/// Order-independence matters because the two sides being compared —
/// re-fetched raw ledgers and stored derived rows — have no guaranteed
/// common iteration order; a checksum sensitive to order would flag drift
/// on every run regardless of whether the data actually matched. Wrapping
/// addition (rather than XOR) is deliberate: XOR-combining would let two
/// occurrences of the same row cancel out, silently hiding exactly the
/// double-counting bug this check exists to catch.
fn checksum(rows: &[RawSnapshotRow]) -> u64 {
    rows.iter().fold(0u64, |acc, row| {
        let canonical = format!(
            "{}|{}|{}|{}|{}|{}",
            row.ledger_sequence,
            row.source,
            row.corridor,
            row.reliability,
            row.volume,
            row.latency_ms
        );
        acc.wrapping_add(fnv1a_u64(&canonical))
    })
}

/// Periodically checks a trailing window of already-ingested ledgers for
/// drift between the derived store and a fresh recomputation from raw data.
pub struct ReconciliationCheck<L, W, D, A> {
    source: L,
    watermark: W,
    derived: D,
    alerts: A,
    window: u64,
}

impl<L, W, D, A> ReconciliationCheck<L, W, D, A>
where
    L: LedgerSource,
    W: WatermarkStore,
    D: DerivedStore,
    A: DriftAlertSink,
{
    /// `window` is how many trailing ledgers each check covers.
    pub fn new(source: L, watermark: W, derived: D, alerts: A, window: u64) -> Self {
        Self {
            source,
            watermark,
            derived,
            alerts,
            window: window.max(1),
        }
    }

    /// Checks the trailing window ending at the current watermark. Returns
    /// `Ok(None)` when nothing has been ingested yet, or when the window
    /// matched; returns the alert (already emitted) on drift.
    pub async fn run_once(&self) -> Result<Option<DriftAlert>, IngestionError> {
        let Some(watermark) = self.watermark.load().await? else {
            return Ok(None);
        };

        let end = watermark.sequence;
        let start = end.saturating_sub(self.window - 1).max(1);

        let mut raw_rows = Vec::new();
        for sequence in start..=end {
            if let Some(ledger) = self.source.fetch_ledger(sequence).await? {
                raw_rows.extend(ledger.rows);
            }
        }

        let stored_rows = self.derived.rows_in_range(start, end).await?;

        let raw_checksum = checksum(&raw_rows);
        let derived_checksum = checksum(&stored_rows);

        if raw_checksum == derived_checksum {
            return Ok(None);
        }

        let alert = DriftAlert {
            window_start: start,
            window_end: end,
            raw_checksum,
            derived_checksum,
        };
        self.alerts.emit(alert.clone()).await?;
        Ok(Some(alert))
    }

    /// Runs [`Self::run_once`] on a fixed interval, forever. A failed check
    /// is logged and retried next tick rather than ending the loop — a
    /// transient fetch failure checking last week's data is not a reason to
    /// stop checking tomorrow's.
    pub async fn run_forever(&self, interval: Duration) {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            if let Err(error) = self.run_once().await {
                tracing::warn!(error = %error, "reconciliation check failed; retrying next tick");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::fetch::FakeLedgerSource;
    use crate::ingestion::upsert::InMemoryDerivedStore;
    use crate::ingestion::watermark::{InMemoryWatermarkStore, Watermark};

    fn row(sequence: u64, corridor: &str, volume: f64) -> RawSnapshotRow {
        RawSnapshotRow {
            ledger_sequence: sequence,
            corridor: corridor.to_string(),
            source: "testnet".to_string(),
            reliability: 1.0,
            volume,
            latency_ms: 5.0,
        }
    }

    async fn setup(
        window: u64,
    ) -> ReconciliationCheck<
        FakeLedgerSource,
        InMemoryWatermarkStore,
        InMemoryDerivedStore,
        InMemoryDriftAlertSink,
    > {
        let source = FakeLedgerSource::new();
        for seq in 1..=5u64 {
            source
                .append("a", vec![row(seq, "eur/usd", seq as f64)])
                .await;
        }

        let watermark = InMemoryWatermarkStore::new();
        watermark
            .save(Watermark {
                sequence: 5,
                hash: "irrelevant-for-this-check".to_string(),
            })
            .await
            .unwrap();

        let derived = InMemoryDerivedStore::new();
        for seq in 1..=5u64 {
            derived
                .upsert(seq, vec![row(seq, "eur/usd", seq as f64)])
                .await
                .unwrap();
        }

        ReconciliationCheck::new(
            source,
            watermark,
            derived,
            InMemoryDriftAlertSink::new(),
            window,
        )
    }

    #[tokio::test]
    async fn matching_raw_and_derived_data_raises_no_alert() {
        let check = setup(5).await;
        assert_eq!(check.run_once().await.unwrap(), None);
        assert!(check.alerts.alerts().await.is_empty());
    }

    #[tokio::test]
    async fn no_watermark_means_nothing_to_check_yet() {
        let source = FakeLedgerSource::new();
        let check = ReconciliationCheck::new(
            source,
            InMemoryWatermarkStore::new(),
            InMemoryDerivedStore::new(),
            InMemoryDriftAlertSink::new(),
            5,
        );

        assert_eq!(check.run_once().await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_stale_derived_row_is_detected_and_alerted() {
        let check = setup(5).await;
        // Simulate the derived store having drifted from raw ledger data —
        // e.g. a bug in the ingestion write path corrupted one row.
        check
            .derived
            .upsert(3, vec![row(3, "eur/usd", 999.0)])
            .await
            .unwrap();

        let alert = check
            .run_once()
            .await
            .unwrap()
            .expect("drift must be detected");
        assert_eq!(alert.window_start, 1);
        assert_eq!(alert.window_end, 5);
        assert_ne!(alert.raw_checksum, alert.derived_checksum);
        assert_eq!(check.alerts.alerts().await, vec![alert]);
    }

    #[tokio::test]
    async fn a_missing_derived_row_is_detected() {
        let check = setup(5).await;
        check.derived.invalidate_from(5).await.unwrap();

        assert!(check.run_once().await.unwrap().is_some());
    }

    #[test]
    fn checksum_is_order_independent() {
        let a = vec![row(1, "eur/usd", 1.0), row(2, "btc/usd", 2.0)];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(checksum(&a), checksum(&b));
    }

    #[test]
    fn checksum_changes_when_a_row_is_duplicated() {
        // A checksum built from XOR would let a duplicated row cancel out
        // and silently pass, hiding exactly the double-counting bug this
        // check exists to catch; wrapping addition must not have that
        // problem.
        let single = vec![row(1, "eur/usd", 1.0)];
        let mut doubled = single.clone();
        doubled.push(row(1, "eur/usd", 1.0));

        assert_ne!(
            checksum(&single),
            checksum(&doubled),
            "a duplicated row must change the checksum, not cancel out"
        );
    }

    #[tokio::test]
    async fn window_is_clamped_to_at_least_one() {
        let check = setup(0).await;
        assert_eq!(check.window, 1);
    }
}
