//! Comprehensive integration tests for the Payment Reliability and Latency Percentile Computation Engine.
//!
//! Validates:
//! 1. Bounded error percentiles (P50/P95/P99) vs exact percentiles on multiple distributions.
//! 2. Out-of-order event arrival handling per explicit watermarking policy.
//! 3. Two clocks (ledger consensus time vs ingestion wall time) and latency clock basis.
//! 4. Consistency with the reconciliation subsystem's notion of finalized periods.
//! 5. Backpressure and ingestion burst resilience under high-volume workloads.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use stellar_insights_backend::analytics::{
    DDSketch, ExactSummary, IngestOutcome, LateEventPolicy, PaymentAnalyticsEngine,
    PaymentCorridor, PaymentEvent, PaymentStatus, WatermarkConfig, WatermarkedAggregateStore,
    WindowState,
};
use stellar_insights_backend::reconciliation::{
    AgreementSpec, AlertEvent, AlertSink, OffChainAggregateStore, OnChainSnapshotReader,
    OnChainSnapshotView, ReconciliationError, ReconciliationJob,
};

/// In-memory alert sink for testing reconciliation.
#[derive(Default, Clone)]
struct TestAlertSink {
    alerts: Arc<std::sync::Mutex<Vec<AlertEvent>>>,
}

#[async_trait]
impl AlertSink for TestAlertSink {
    async fn emit(&self, event: AlertEvent) -> Result<(), ReconciliationError> {
        self.alerts.lock().unwrap().push(event);
        Ok(())
    }
}

/// Mock on-chain reader providing matching snapshots.
struct MockOnChainReader {
    snapshots: std::collections::HashMap<u64, OnChainSnapshotView>,
}

#[async_trait]
impl OnChainSnapshotReader for MockOnChainReader {
    async fn get_snapshot(
        &self,
        period: u64,
    ) -> Result<Option<OnChainSnapshotView>, ReconciliationError> {
        Ok(self.snapshots.get(&period).cloned())
    }
}

// ---------------------------------------------------------------------------
// TEST 1: DDSketch Error Bound vs Exact Ground Truth Across Diverse Distributions
// ---------------------------------------------------------------------------
#[test]
fn test_sketch_error_bound_against_exact_distributions() {
    let alpha = 0.01; // 1% relative error guarantee
    let mut sketch = DDSketch::new(alpha).expect("valid alpha");
    let mut exact = ExactSummary::new();

    // Generate 10,000 synthetic payment latency samples with a bimodal log-normal distribution
    // (representing typical fast payments ~100-300ms and slow multi-hop cross-border corridor payments ~2000-8000ms)
    for i in 1..=10_000 {
        let base = if i % 4 == 0 {
            // Slow corridor payment
            ((i as f64) * 0.123).sin().abs() * 5000.0 + 2000.0
        } else {
            // Fast payment
            ((i as f64) * 0.456).cos().abs() * 250.0 + 50.0
        };

        sketch.add(base).unwrap();
        exact.add(base);
    }

    assert_eq!(sketch.count(), 10_000);
    assert_eq!(exact.count(), 10_000);

    let test_quantiles = [0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99, 0.999];

    for &q in &test_quantiles {
        let exact_q = exact.quantile(q).expect("exact quantile available");
        let sketch_q = sketch.quantile(q).expect("sketch quantile available");

        let relative_error = (sketch_q - exact_q).abs() / exact_q;

        assert!(
            relative_error <= alpha + 1e-4,
            "Quantile q={}: exact={}, sketch={}, rel_err={} > alpha={}",
            q,
            exact_q,
            sketch_q,
            relative_error,
            alpha
        );
    }

    // Verify PercentileSummary struct
    let summary = sketch.summary().expect("summary available");
    assert_eq!(summary.count, 10_000);
    assert!(summary.p50 > 0.0);
    assert!(summary.p95 >= summary.p50);
    assert!(summary.p99 >= summary.p95);
    assert!(summary.p999 >= summary.p99);
}

// ---------------------------------------------------------------------------
// TEST 2: Out-of-Order Event Arrival with Watermarking Policy
// ---------------------------------------------------------------------------
#[test]
fn test_out_of_order_stream_watermarking_and_convergence() {
    // Watermark configured with 30s tolerance
    let config = WatermarkConfig {
        window_size_secs: 60,
        watermark_delay_secs: 30,
        late_event_policy: LateEventPolicy::DropAndRecord,
        ..Default::default()
    };

    // Ingest in-order stream into engine A
    let engine_in_order = PaymentAnalyticsEngine::new(config.clone());
    // Ingest locally jittered / out-of-order stream into engine B
    let engine_shuffled = PaymentAnalyticsEngine::new(config);

    // Create 1,000 events within timestamps 1..=120 with bounded out-of-order jitter (<= 15s)
    let mut events = Vec::new();
    for i in 1..=1000 {
        let base_t = ((i as u64) * 110) / 1000 + 1; // 1..111
        let jitter = ((i * 7) % 11) as u64; // 0..10s jitter
        let t = (base_t + jitter).min(115);
        events.push(PaymentEvent {
            payment_id: format!("tx_{}", i),
            ledger_sequence: i as u64,
            ledger_closed_at: t,
            client_submitted_at: Some(t.saturating_sub(1)),
            ingested_at: t + 1,
            status: if i % 10 == 0 {
                PaymentStatus::Failed
            } else {
                PaymentStatus::Success
            },
            corridor: Some(PaymentCorridor::new("XLM", "USDC")),
            latency_ms: Some(((i % 100) as f64) * 10.0 + 50.0),
        });
    }

    // Engine A: in-order (sorted by ledger_closed_at)
    let mut in_order_events = events.clone();
    in_order_events.sort_by_key(|e| e.ledger_closed_at);
    for e in in_order_events {
        engine_in_order.ingest(e);
    }

    // Engine B: shuffled out-of-order arrival (arrival within watermark delay horizon)
    for e in events {
        engine_shuffled.ingest(e);
    }

    // Advance watermark on both past window 0 (to timestamp 150 -> watermark 130 > 60)
    let terminal_event = PaymentEvent {
        payment_id: "tx_term".into(),
        ledger_sequence: 9999,
        ledger_closed_at: 150,
        client_submitted_at: None,
        ingested_at: 151,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(100.0),
    };
    engine_in_order.ingest(terminal_event.clone());
    engine_shuffled.ingest(terminal_event);

    let w0_in_order = engine_in_order.get_window(0).expect("window 0 in order");
    let w0_shuffled = engine_shuffled.get_window(0).expect("window 0 shuffled");

    assert_eq!(w0_in_order.state, WindowState::Finalized);
    assert_eq!(w0_shuffled.state, WindowState::Finalized);
    assert_eq!(w0_in_order.sketch.count(), w0_shuffled.sketch.count());
    assert_eq!(
        w0_in_order.reliability.total_payments,
        w0_shuffled.reliability.total_payments
    );
    assert_eq!(
        w0_in_order.reliability.successful_payments,
        w0_shuffled.reliability.successful_payments
    );

    // Percentiles should match exactly between in-order and out-of-order streams
    let p_in = w0_in_order.percentiles().unwrap();
    let p_shuf = w0_shuffled.percentiles().unwrap();

    assert!((p_in.p50 - p_shuf.p50).abs() < 1e-6);
    assert!((p_in.p95 - p_shuf.p95).abs() < 1e-6);
    assert!((p_in.p99 - p_shuf.p99).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// TEST 3: Late Event Handling Policies (DropAndRecord, SideOutput, RetroactiveUpdate)
// ---------------------------------------------------------------------------
#[test]
fn test_late_event_policies() {
    // 3a. DropAndRecord Policy
    let config_drop = WatermarkConfig {
        window_size_secs: 60,
        watermark_delay_secs: 10,
        late_event_policy: LateEventPolicy::DropAndRecord,
        ..Default::default()
    };
    let engine_drop = PaymentAnalyticsEngine::new(config_drop);

    // Advance watermark to 90 (finalizing window 0: 0..60)
    engine_drop.ingest(PaymentEvent {
        payment_id: "tx_100".into(),
        ledger_sequence: 100,
        ledger_closed_at: 100,
        client_submitted_at: None,
        ingested_at: 100,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(100.0),
    });
    assert_eq!(engine_drop.current_watermark(), 90);

    // Late event arriving with t=20
    let outcome_drop = engine_drop.ingest(PaymentEvent {
        payment_id: "late_20".into(),
        ledger_sequence: 20,
        ledger_closed_at: 20,
        client_submitted_at: None,
        ingested_at: 105,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(500.0),
    });

    assert_eq!(
        outcome_drop,
        IngestOutcome::LateEventHandled {
            window_id: 0,
            policy: LateEventPolicy::DropAndRecord,
        }
    );
    assert_eq!(engine_drop.summary().total_late_events, 1);

    // 3b. SideOutput Policy
    let config_side = WatermarkConfig {
        window_size_secs: 60,
        watermark_delay_secs: 10,
        late_event_policy: LateEventPolicy::SideOutput,
        ..Default::default()
    };
    let engine_side = PaymentAnalyticsEngine::new(config_side);
    engine_side.ingest(PaymentEvent {
        payment_id: "tx_100".into(),
        ledger_sequence: 100,
        ledger_closed_at: 100,
        client_submitted_at: None,
        ingested_at: 100,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(100.0),
    });

    let outcome_side = engine_side.ingest(PaymentEvent {
        payment_id: "late_20_side".into(),
        ledger_sequence: 20,
        ledger_closed_at: 20,
        client_submitted_at: None,
        ingested_at: 105,
        status: PaymentStatus::Failed,
        corridor: None,
        latency_ms: Some(500.0),
    });

    assert_eq!(
        outcome_side,
        IngestOutcome::LateEventHandled {
            window_id: 0,
            policy: LateEventPolicy::SideOutput,
        }
    );

    // 3c. RetroactiveUpdate Policy
    let config_retro = WatermarkConfig {
        window_size_secs: 60,
        watermark_delay_secs: 10,
        late_event_policy: LateEventPolicy::RetroactiveUpdate,
        ..Default::default()
    };
    let engine_retro = PaymentAnalyticsEngine::new(config_retro);

    // Initial event in window 0
    engine_retro.ingest(PaymentEvent {
        payment_id: "tx_10".into(),
        ledger_sequence: 10,
        ledger_closed_at: 10,
        client_submitted_at: None,
        ingested_at: 10,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(100.0),
    });

    // Advance watermark past window 0
    engine_retro.ingest(PaymentEvent {
        payment_id: "tx_100".into(),
        ledger_sequence: 100,
        ledger_closed_at: 100,
        client_submitted_at: None,
        ingested_at: 100,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(100.0),
    });

    let w0 = engine_retro.get_window(0).unwrap();
    assert_eq!(w0.state, WindowState::Finalized);
    assert_eq!(w0.revision, 1);
    assert_eq!(w0.sketch.count(), 1);

    // Late event retroactively updates window 0
    let outcome_retro = engine_retro.ingest(PaymentEvent {
        payment_id: "late_20_retro".into(),
        ledger_sequence: 20,
        ledger_closed_at: 20,
        client_submitted_at: None,
        ingested_at: 110,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(250.0),
    });

    assert_eq!(
        outcome_retro,
        IngestOutcome::LateEventHandled {
            window_id: 0,
            policy: LateEventPolicy::RetroactiveUpdate,
        }
    );

    let w0_amended = engine_retro.get_window(0).unwrap();
    assert_eq!(w0_amended.state, WindowState::Amended);
    assert_eq!(w0_amended.revision, 2);
    assert_eq!(w0_amended.sketch.count(), 2);
}

// ---------------------------------------------------------------------------
// TEST 4: Two Clocks, Ingestion Lag, and Clock Skew
// ---------------------------------------------------------------------------
#[test]
fn test_clock_separation_and_skew_detection() {
    let engine = PaymentAnalyticsEngine::with_default_config();

    // Event with normal timing
    let normal_event = PaymentEvent {
        payment_id: "normal".into(),
        ledger_sequence: 10,
        ledger_closed_at: 1700000000,
        client_submitted_at: Some(1700000000 - 3),
        ingested_at: 1700000002, // 2s ingestion lag
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: None,
    };

    assert_eq!(normal_event.effective_latency_ms(5000.0), 3000.0);
    assert_eq!(normal_event.ingestion_lag_secs(), 2);
    assert!(!normal_event.is_clock_skewed(60));

    let outcome_normal = engine.ingest(normal_event);
    assert!(matches!(outcome_normal, IngestOutcome::Incorporated { .. }));

    // Event with anomalous clock skew (ledger time far in the future compared to wall-clock ingestion time)
    let skewed_event = PaymentEvent {
        payment_id: "skewed".into(),
        ledger_sequence: 20,
        ledger_closed_at: 1700000500, // 500s ahead
        client_submitted_at: None,
        ingested_at: 1700000000,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(120.0),
    };

    assert!(skewed_event.is_clock_skewed(60));
    let outcome_skewed = engine.ingest(skewed_event);
    assert!(matches!(
        outcome_skewed,
        IngestOutcome::ClockSkewFlagged { .. }
    ));
    assert_eq!(engine.summary().total_clock_skew_events, 1);
}

// ---------------------------------------------------------------------------
// TEST 5: Integration and Agreement with Reconciliation Subsystem
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reconciliation_subsystem_agreement() {
    let config = WatermarkConfig {
        window_size_secs: 60,
        watermark_delay_secs: 15,
        late_event_policy: LateEventPolicy::DropAndRecord,
        ..Default::default()
    };

    let engine = Arc::new(PaymentAnalyticsEngine::new(config));
    let bridge = Arc::new(WatermarkedAggregateStore::new(engine.clone()));
    let alert_sink = Arc::new(TestAlertSink::default());

    // Ingest events into window 0 (t=10) and window 1 (t=70)
    engine.ingest(PaymentEvent {
        payment_id: "tx_10".into(),
        ledger_sequence: 1,
        ledger_closed_at: 10,
        client_submitted_at: Some(9),
        ingested_at: 11,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(100.0),
    });

    engine.ingest(PaymentEvent {
        payment_id: "tx_70".into(),
        ledger_sequence: 2,
        ledger_closed_at: 70,
        client_submitted_at: Some(69),
        ingested_at: 71,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(150.0),
    });

    // Advance watermark to 85 (t=100)
    engine.ingest(PaymentEvent {
        payment_id: "tx_100".into(),
        ledger_sequence: 3,
        ledger_closed_at: 100,
        client_submitted_at: Some(99),
        ingested_at: 101,
        status: PaymentStatus::Success,
        corridor: None,
        latency_ms: Some(120.0),
    });

    // At watermark=85:
    // Window 0 (0..60) end 60 <= 85 -> Finalized & Reconcilable
    // Window 1 (60..120) end 120 > 85 -> Active & NOT reconcilable
    let reconcilable_periods = bridge.reconcilable_periods().await.unwrap();
    assert_eq!(reconcilable_periods, vec![0]);

    let offchain_agg_0 = bridge
        .get_aggregate(0)
        .await
        .unwrap()
        .expect("period 0 aggregate exists");

    // Provide matching on-chain snapshot for period 0
    let mut onchain_snapshots = std::collections::HashMap::new();
    onchain_snapshots.insert(
        0,
        OnChainSnapshotView {
            period: 0,
            snapshot_hash: offchain_agg_0.snapshot_hash,
            source_data_hash: offchain_agg_0.source_data_hash,
        },
    );

    let onchain_reader = Arc::new(MockOnChainReader {
        snapshots: onchain_snapshots,
    });

    let spec = AgreementSpec::default();
    let job = ReconciliationJob::new(spec, bridge.clone(), onchain_reader, alert_sink.clone());

    let report = job.run_once().await.expect("reconciliation job succeeds");

    assert_eq!(report.checked_periods, 1);
    assert_eq!(report.discrepancies.len(), 0);
    assert!(alert_sink.alerts.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// TEST 6: Ingestion Burst Resilience & Backpressure Safety
// ---------------------------------------------------------------------------
#[test]
fn test_simulated_ingestion_burst_preserves_correctness() {
    let config = WatermarkConfig {
        window_size_secs: 60,
        watermark_delay_secs: 15,
        late_event_policy: LateEventPolicy::DropAndRecord,
        ..Default::default()
    };
    let engine = PaymentAnalyticsEngine::new(config);

    // Simulate an indexer backlog replay of 50,000 events arriving in a sudden burst
    let burst_size = 50_000;
    let mut burst_events = Vec::with_capacity(burst_size);

    for i in 0..burst_size {
        // Event timestamps spread across 10 windows (0..600 seconds)
        let t = ((i * 37) % 600) as u64 + 1;
        burst_events.push(PaymentEvent {
            payment_id: format!("burst_{}", i),
            ledger_sequence: i as u64,
            ledger_closed_at: t,
            client_submitted_at: Some(t.saturating_sub(1)),
            ingested_at: t + 2,
            status: if i % 50 == 0 {
                PaymentStatus::Failed
            } else {
                PaymentStatus::Success
            },
            corridor: Some(PaymentCorridor::new("XLM", "EURC")),
            latency_ms: Some(((i % 1000) as f64) * 2.0 + 10.0),
        });
    }

    burst_events.sort_by_key(|e| e.ledger_closed_at);

    let start_time = SystemTime::now();
    let batch_result = engine.ingest_batch(burst_events);
    let duration = start_time.elapsed().unwrap();

    assert_eq!(batch_result.total_ingested, burst_size);
    println!(
        "Processed {} events in {:?} ({:.0} events/sec)",
        burst_size,
        duration,
        (burst_size as f64) / duration.as_secs_f64()
    );

    let summary = engine.summary();
    assert!(summary.max_event_time >= 600);
    assert!(summary.current_watermark >= 585);

    // All windows 0 through 8 should be finalized (end_time <= 585)
    let finalized_windows = engine.finalized_windows();
    assert!(finalized_windows.len() >= 8);

    for window in finalized_windows {
        assert_eq!(window.state, WindowState::Finalized);
        let p = window.percentiles().expect("percentiles present");
        assert!(p.count > 0);
        assert!(p.p50 > 0.0);
        assert!(p.p95 >= p.p50);
        assert!(p.p99 >= p.p95);

        let r = window.reliability_summary();
        assert!(r.total_payments > 0);
        assert!(r.success_rate >= 0.95);
        assert!(r.availability_percent >= 95.0);
    }
}
