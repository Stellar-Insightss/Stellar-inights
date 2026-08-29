//! Bridge adapter connecting `PaymentAnalyticsEngine` to the `reconciliation` subsystem.
//!
//! # Consistency with Reconciliation Subsystem's Period Finality
//!
//! The reconciliation subsystem (`backend/src/reconciliation/`) requires an [`OffChainAggregateStore`]
//! to supply finalized, reconcilable periods and their cryptographic snapshot hashes.
//!
//! [`WatermarkedAggregateStore`] wraps [`PaymentAnalyticsEngine`] and directly implements
//! [`OffChainAggregateStore`]:
//! - `reconcilable_periods()` returns only periods where $T_{\text{end}} \le W(t)$ (strictly finalized).
//! - `get_aggregate(period)` returns an [`OffChainAggregate`] containing the deterministic
//!   `snapshot_hash` and `source_data_hash` computed over the window's DDSketch and reliability state.
//!
//! This ensures that analytics and reconciliation share the exact same definition of "finalized".

use async_trait::async_trait;
use std::sync::Arc;

use crate::analytics::engine::PaymentAnalyticsEngine;
use crate::reconciliation::{OffChainAggregate, OffChainAggregateStore, ReconciliationError};

/// Store adapter bridging the payment analytics engine to the reconciliation subsystem.
#[derive(Clone)]
pub struct WatermarkedAggregateStore {
    engine: Arc<PaymentAnalyticsEngine>,
}

impl WatermarkedAggregateStore {
    /// Creates a new bridge store wrapping the payment analytics engine.
    pub fn new(engine: Arc<PaymentAnalyticsEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl OffChainAggregateStore for WatermarkedAggregateStore {
    async fn reconcilable_periods(&self) -> Result<Vec<u64>, ReconciliationError> {
        Ok(self.engine.reconcilable_periods())
    }

    async fn get_aggregate(
        &self,
        period: u64,
    ) -> Result<Option<OffChainAggregate>, ReconciliationError> {
        let window = match self.engine.get_window(period) {
            Some(w) => w,
            None => return Ok(None),
        };

        // Only finalized or amended windows are eligible for reconciliation
        if window.state == crate::analytics::watermark::WindowState::Active {
            return Ok(None);
        }

        Ok(Some(OffChainAggregate {
            period,
            snapshot_hash: window.snapshot_hash(),
            source_data_hash: window.source_data_hash(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::clock::PaymentEvent;
    use crate::analytics::reliability::PaymentStatus;
    use crate::analytics::watermark::{LateEventPolicy, WatermarkConfig};

    #[tokio::test]
    async fn test_watermarked_aggregate_store_reconciliation() {
        let config = WatermarkConfig {
            window_size_secs: 60,
            watermark_delay_secs: 15,
            late_event_policy: LateEventPolicy::DropAndRecord,
            ..Default::default()
        };

        let engine = Arc::new(PaymentAnalyticsEngine::new(config));
        let bridge = WatermarkedAggregateStore::new(engine.clone());

        // Ingest event in Window 0 (0..60)
        engine.ingest(PaymentEvent {
            payment_id: "tx_10".into(),
            ledger_sequence: 1,
            ledger_closed_at: 10,
            client_submitted_at: Some(9),
            ingested_at: 11,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(150.0),
        });

        // Window 0 is not yet finalized (watermark = 0)
        let periods_before = bridge.reconcilable_periods().await.unwrap();
        assert!(periods_before.is_empty());
        let agg_before = bridge.get_aggregate(0).await.unwrap();
        assert_eq!(agg_before, None);

        // Advance watermark to 85 (ingesting at t=100)
        engine.ingest(PaymentEvent {
            payment_id: "tx_100".into(),
            ledger_sequence: 2,
            ledger_closed_at: 100,
            client_submitted_at: Some(99),
            ingested_at: 101,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(120.0),
        });

        // Window 0 is now finalized (60 <= 85)
        let periods_after = bridge.reconcilable_periods().await.unwrap();
        assert_eq!(periods_after, vec![0]);

        let agg = bridge.get_aggregate(0).await.unwrap().expect("Aggregate present");
        assert_eq!(agg.period, 0);
        assert_ne!(agg.snapshot_hash, [0u8; 32]);
        assert_ne!(agg.source_data_hash, [0u8; 32]);
    }
}
