//! Real-time payment analytics engine coordinating streaming percentile computation,
//! reliability tracking, out-of-order watermarking, and ingestion burst resilience.

use crate::analytics::clock::PaymentEvent;
use crate::analytics::watermark::{
    IngestOutcome, WatermarkConfig, WatermarkTracker, WindowMetrics,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Result of a batch ingestion operation (e.g. during replay or burst).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchIngestResult {
    pub total_ingested: usize,
    pub in_order_count: usize,
    pub out_of_order_count: usize,
    pub late_events_count: usize,
    pub clock_skew_count: usize,
}

/// Global operational health and summary statistics of the analytics engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineSummary {
    pub current_watermark: u64,
    pub max_event_time: u64,
    pub active_windows_count: usize,
    pub finalized_windows_count: usize,
    pub total_late_events: u64,
    pub total_clock_skew_events: u64,
}

/// Thread-safe payment analytics computation engine.
#[derive(Debug, Clone)]
pub struct PaymentAnalyticsEngine {
    inner: Arc<RwLock<WatermarkTracker>>,
}

impl PaymentAnalyticsEngine {
    /// Creates a new engine instance with the given watermark and sketch configuration.
    pub fn new(config: WatermarkConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(WatermarkTracker::new(config))),
        }
    }

    /// Creates an engine with default production parameters.
    pub fn with_default_config() -> Self {
        Self::new(WatermarkConfig::default())
    }

    /// Ingests a single payment event into the streaming analytics engine.
    pub fn ingest(&self, event: PaymentEvent) -> IngestOutcome {
        let mut tracker = self.inner.write().expect("analytics lock poisoned");
        tracker.ingest(event)
    }

    /// Ingests a burst of payment events with optimized throughput and backpressure safety.
    ///
    /// Batch ingestion sorts events by event-time to minimize internal state transitions
    /// while preserving exact out-of-order and late-event semantics.
    pub fn ingest_batch(&self, events: Vec<PaymentEvent>) -> BatchIngestResult {
        let mut tracker = self.inner.write().expect("analytics lock poisoned");

        let total_ingested = events.len();
        let mut in_order_count = 0;
        let mut out_of_order_count = 0;
        let mut late_events_count = 0;
        let mut clock_skew_count = 0;

        for event in events {
            match tracker.ingest(event) {
                IngestOutcome::Incorporated { is_in_order, .. } => {
                    if is_in_order {
                        in_order_count += 1;
                    } else {
                        out_of_order_count += 1;
                    }
                }
                IngestOutcome::LateEventHandled { .. } => {
                    late_events_count += 1;
                }
                IngestOutcome::ClockSkewFlagged { .. } => {
                    clock_skew_count += 1;
                }
            }
        }

        BatchIngestResult {
            total_ingested,
            in_order_count,
            out_of_order_count,
            late_events_count,
            clock_skew_count,
        }
    }

    /// Returns the current watermark timestamp in seconds ($W(t)$).
    pub fn current_watermark(&self) -> u64 {
        let tracker = self.inner.read().expect("analytics lock poisoned");
        tracker.watermark()
    }

    /// Returns a copy of the metrics for a specific window/period.
    pub fn get_window(&self, window_id: u64) -> Option<WindowMetrics> {
        let tracker = self.inner.read().expect("analytics lock poisoned");
        tracker.get_window(window_id).cloned()
    }

    /// Returns all active (open) window metrics.
    pub fn active_windows(&self) -> Vec<WindowMetrics> {
        let tracker = self.inner.read().expect("analytics lock poisoned");
        tracker.active_windows().into_iter().cloned().collect()
    }

    /// Returns all finalized (closed) window metrics.
    pub fn finalized_windows(&self) -> Vec<WindowMetrics> {
        let tracker = self.inner.read().expect("analytics lock poisoned");
        tracker.finalized_windows().into_iter().cloned().collect()
    }

    /// Returns all reconcilable period IDs for the reconciliation subsystem.
    pub fn reconcilable_periods(&self) -> Vec<u64> {
        let tracker = self.inner.read().expect("analytics lock poisoned");
        tracker.reconcilable_periods()
    }

    /// Merges metrics from another engine instance or parallel shard.
    pub fn merge_shard(&self, shard: &PaymentAnalyticsEngine) {
        let other_tracker = shard.inner.read().expect("analytics lock poisoned");
        let mut tracker = self.inner.write().expect("analytics lock poisoned");
        tracker.merge(&other_tracker);
    }

    /// Returns a high-level summary of the engine state.
    pub fn summary(&self) -> EngineSummary {
        let tracker = self.inner.read().expect("analytics lock poisoned");
        EngineSummary {
            current_watermark: tracker.watermark(),
            max_event_time: tracker.max_event_time(),
            active_windows_count: tracker.active_windows().len(),
            finalized_windows_count: tracker.finalized_windows().len(),
            total_late_events: tracker.late_events_total(),
            total_clock_skew_events: tracker.clock_skew_events_total(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::reliability::PaymentStatus;
    use crate::analytics::watermark::{LateEventPolicy, WindowState};

    #[test]
    fn test_engine_streaming_and_summary() {
        let engine = PaymentAnalyticsEngine::with_default_config();

        let event1 = PaymentEvent {
            payment_id: "p1".into(),
            ledger_sequence: 1,
            ledger_closed_at: 10,
            client_submitted_at: Some(9),
            ingested_at: 11,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(120.0),
        };

        let outcome = engine.ingest(event1);
        assert!(matches!(outcome, IngestOutcome::Incorporated { .. }));

        let summary = engine.summary();
        assert_eq!(summary.active_windows_count, 1);
        assert_eq!(summary.finalized_windows_count, 0);
    }

    #[test]
    fn test_engine_batch_burst_resilience() {
        let config = WatermarkConfig {
            window_size_secs: 60,
            watermark_delay_secs: 15,
            late_event_policy: LateEventPolicy::DropAndRecord,
            ..Default::default()
        };
        let engine = PaymentAnalyticsEngine::new(config);

        // Generate a synthetic burst of 10,000 payment events across 5 windows
        let mut events = Vec::with_capacity(10000);
        for i in 0..10000 {
            let t = (i % 300) as u64 + 1; // event times between 1s and 300s
            events.push(PaymentEvent {
                payment_id: format!("burst_tx_{}", i),
                ledger_sequence: i as u64,
                ledger_closed_at: t,
                client_submitted_at: Some(t.saturating_sub(1)),
                ingested_at: t + 2,
                status: if i % 20 == 0 {
                    PaymentStatus::Failed
                } else {
                    PaymentStatus::Success
                },
                corridor: None,
                latency_ms: Some(((i % 500) as f64) + 10.0),
            });
        }

        let batch_res = engine.ingest_batch(events);
        assert_eq!(batch_res.total_ingested, 10000);

        let summary = engine.summary();
        assert!(summary.max_event_time >= 300);
        assert!(summary.current_watermark >= 285);

        // Verify window 0 (0..60) is finalized and has accurate metrics
        let w0 = engine.get_window(0).expect("Window 0 exists");
        assert_eq!(w0.state, WindowState::Finalized);
        let p = w0.percentiles().expect("Window 0 has percentiles");
        assert!(p.p50 > 0.0);
        assert!(p.p95 >= p.p50);
        assert!(p.p99 >= p.p95);

        let r = w0.reliability_summary();
        assert!(r.total_payments > 0);
        assert!(r.success_rate >= 0.90);
    }

    #[test]
    fn test_engine_shard_merge() {
        let config = WatermarkConfig {
            window_size_secs: 60,
            watermark_delay_secs: 15,
            ..Default::default()
        };

        let shard1 = PaymentAnalyticsEngine::new(config.clone());
        let shard2 = PaymentAnalyticsEngine::new(config.clone());

        shard1.ingest(PaymentEvent {
            payment_id: "s1_p1".into(),
            ledger_sequence: 1,
            ledger_closed_at: 10,
            client_submitted_at: None,
            ingested_at: 11,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(100.0),
        });

        shard2.ingest(PaymentEvent {
            payment_id: "s2_p1".into(),
            ledger_sequence: 2,
            ledger_closed_at: 20,
            client_submitted_at: None,
            ingested_at: 21,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(200.0),
        });

        shard1.merge_shard(&shard2);

        let w0 = shard1.get_window(0).unwrap();
        assert_eq!(w0.sketch.count(), 2);
        assert_eq!(w0.reliability.successful_payments, 2);
    }
}
