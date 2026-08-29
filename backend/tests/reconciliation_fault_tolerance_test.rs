use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{watch, Mutex};

use stellar_insights_backend::reconciliation::{
    AgreementSpec, AlertEvent, AlertKind, AlertSink, BackoffConfig, InMemoryReconciliationStore,
    OffChainAggregate, OffChainAggregateStore, OnChainSnapshotReader, OnChainSnapshotView,
    ReconciliationError, ReconciliationJob, ReconciliationStateStore,
};

struct ConfigurableOffChainStore {
    periods: Mutex<Vec<u64>>,
    aggregates: Mutex<HashMap<u64, OffChainAggregate>>,
    fail_reconcilable_periods: AtomicBool,
    fail_periods: Mutex<HashSet<u64>>,
}

impl ConfigurableOffChainStore {
    fn new(periods: Vec<u64>, aggregates: HashMap<u64, OffChainAggregate>) -> Self {
        Self {
            periods: Mutex::new(periods),
            aggregates: Mutex::new(aggregates),
            fail_reconcilable_periods: AtomicBool::new(false),
            fail_periods: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl OffChainAggregateStore for ConfigurableOffChainStore {
    async fn reconcilable_periods(&self) -> Result<Vec<u64>, ReconciliationError> {
        if self.fail_reconcilable_periods.load(Ordering::SeqCst) {
            return Err(ReconciliationError::OffChainStore(
                "reconcilable_periods query failed".to_string(),
            ));
        }
        Ok(self.periods.lock().await.clone())
    }

    async fn get_aggregate(
        &self,
        period: u64,
    ) -> Result<Option<OffChainAggregate>, ReconciliationError> {
        if self.fail_periods.lock().await.contains(&period) {
            return Err(ReconciliationError::OffChainStore(format!(
                "transient DB error fetching aggregate for period {}",
                period
            )));
        }
        Ok(self.aggregates.lock().await.get(&period).cloned())
    }
}

struct ConfigurableOnChainAdapter {
    snapshots: Mutex<HashMap<u64, OnChainSnapshotView>>,
    fail_periods: Mutex<HashSet<u64>>,
}

impl ConfigurableOnChainAdapter {
    fn new(snapshots: HashMap<u64, OnChainSnapshotView>) -> Self {
        Self {
            snapshots: Mutex::new(snapshots),
            fail_periods: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl OnChainSnapshotReader for ConfigurableOnChainAdapter {
    async fn get_snapshot(
        &self,
        period: u64,
    ) -> Result<Option<OnChainSnapshotView>, ReconciliationError> {
        if self.fail_periods.lock().await.contains(&period) {
            return Err(ReconciliationError::OnChainRead(format!(
                "RPC node connection timeout for period {}",
                period
            )));
        }
        Ok(self.snapshots.lock().await.get(&period).cloned())
    }
}

#[derive(Default)]
struct TestAlertSink {
    events: Arc<Mutex<Vec<AlertEvent>>>,
}

#[async_trait]
impl AlertSink for TestAlertSink {
    async fn emit(&self, event: AlertEvent) -> Result<(), ReconciliationError> {
        self.events.lock().await.push(event);
        Ok(())
    }
}

#[tokio::test]
async fn test_single_period_fetch_failure_preserves_sibling_results() {
    let offchain = Arc::new(ConfigurableOffChainStore::new(
        vec![1, 2, 3],
        HashMap::from([
            (
                1,
                OffChainAggregate {
                    period: 1,
                    snapshot_hash: [1; 32],
                    source_data_hash: [1; 32],
                },
            ),
            (
                2,
                OffChainAggregate {
                    period: 2,
                    snapshot_hash: [2; 32],
                    source_data_hash: [2; 32],
                },
            ),
            (
                3,
                OffChainAggregate {
                    period: 3,
                    snapshot_hash: [3; 32],
                    source_data_hash: [3; 32],
                },
            ),
        ]),
    ));

    let onchain = Arc::new(ConfigurableOnChainAdapter::new(HashMap::from([
        (
            1,
            OnChainSnapshotView {
                period: 1,
                snapshot_hash: [1; 32],
                source_data_hash: [1; 32],
            },
        ),
        (
            2,
            OnChainSnapshotView {
                period: 2,
                snapshot_hash: [2; 32],
                source_data_hash: [2; 32],
            },
        ),
        (
            3,
            OnChainSnapshotView {
                period: 3,
                snapshot_hash: [99; 32], // Discrepancy on period 3
                source_data_hash: [3; 32],
            },
        ),
    ])));

    // Make period 2 fail transiently on on-chain read
    onchain.fail_periods.lock().await.insert(2);

    let alerts = Arc::new(TestAlertSink::default());
    let job = ReconciliationJob::new(AgreementSpec, offchain, onchain, alerts.clone());

    let report = job.run_once().await.expect("run_once should succeed");

    assert_eq!(report.checked_periods, 3);
    assert_eq!(report.clean_periods(), vec![1]);
    assert_eq!(report.failed_periods(), vec![2]);
    assert_eq!(report.discrepant_periods(), vec![3]);
    assert_eq!(report.discrepancies.len(), 1);
    assert_eq!(report.discrepancies[0].period, 3);

    // Verify alerts: 1 alert for period 2 failure, 1 alert for period 3 discrepancy
    let events = alerts.events.lock().await;
    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|e| e.kind == AlertKind::ReconciliationPeriodFailed && e.period == 2));
    assert!(events.iter().any(|e| e.kind == AlertKind::DiscrepancyDetected && e.period == 3));

    // Verify state store recorded failed period 2
    let retries = job.state_store().load_pending_retries().await.unwrap();
    assert_eq!(retries, vec![2]);
}

#[tokio::test]
async fn test_failed_periods_are_retried_and_cleared_on_recovery() {
    let offchain = Arc::new(ConfigurableOffChainStore::new(
        vec![5],
        HashMap::from([(
            5,
            OffChainAggregate {
                period: 5,
                snapshot_hash: [5; 32],
                source_data_hash: [5; 32],
            },
        )]),
    ));

    let onchain = Arc::new(ConfigurableOnChainAdapter::new(HashMap::from([(
        5,
        OnChainSnapshotView {
            period: 5,
            snapshot_hash: [5; 32],
            source_data_hash: [5; 32],
        },
    )])));

    // Fail period 5 on offchain fetch
    offchain.fail_periods.lock().await.insert(5);

    let alerts = Arc::new(TestAlertSink::default());
    let job = ReconciliationJob::new(AgreementSpec, offchain.clone(), onchain.clone(), alerts.clone());

    // Tick 1: Period 5 fails
    let report1 = job.run_once().await.expect("tick 1 should run");
    assert_eq!(report1.failed_periods(), vec![5]);
    assert!(job.state_store().load_pending_retries().await.unwrap().contains(&5));

    // Now remove failure condition
    offchain.fail_periods.lock().await.remove(&5);

    // Tick 2: Period 5 is retried and succeeds
    let report2 = job.run_once().await.expect("tick 2 should run");
    assert_eq!(report2.clean_periods(), vec![5]);
    assert_eq!(report2.failed_periods().len(), 0);

    // Backlog should now be empty
    assert!(job.state_store().load_pending_retries().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_retry_backlog_survives_process_restart() {
    // Shared persistent state store (e.g. simulated DB or shared instance across restart)
    let persistent_store: Arc<dyn ReconciliationStateStore> =
        Arc::new(InMemoryReconciliationStore::new());

    let offchain = Arc::new(ConfigurableOffChainStore::new(
        vec![10],
        HashMap::from([(
            10,
            OffChainAggregate {
                period: 10,
                snapshot_hash: [10; 32],
                source_data_hash: [10; 32],
            },
        )]),
    ));

    let onchain = Arc::new(ConfigurableOnChainAdapter::new(HashMap::from([(
        10,
        OnChainSnapshotView {
            period: 10,
            snapshot_hash: [10; 32],
            source_data_hash: [10; 32],
        },
    )])));

    // Outage occurs
    onchain.fail_periods.lock().await.insert(10);

    let alerts = Arc::new(TestAlertSink::default());

    // Process 1 (Job 1)
    {
        let job1 = ReconciliationJob::new(
            AgreementSpec,
            offchain.clone(),
            onchain.clone(),
            alerts.clone(),
        )
        .with_state_store(persistent_store.clone());

        let report = job1.run_once().await.unwrap();
        assert_eq!(report.failed_periods(), vec![10]);
    } // job1 dropped, simulating process crash / termination

    // Verify persistent store retained the failed period 10 across the process shutdown
    assert_eq!(persistent_store.load_pending_retries().await.unwrap(), vec![10]);

    // Outage resolves
    onchain.fail_periods.lock().await.remove(&10);

    // Process 2 (Job 2) boots up with the same persistent state store
    let job2 = ReconciliationJob::new(
        AgreementSpec,
        offchain.clone(),
        onchain.clone(),
        alerts.clone(),
    )
    .with_state_store(persistent_store.clone());

    // Job 2 runs its first reconciliation tick
    let report2 = job2.run_once().await.unwrap();
    assert_eq!(report2.clean_periods(), vec![10]);
    assert!(report2.failed_periods().is_empty());

    // Pending retries is now empty in the persistent store
    assert!(persistent_store.load_pending_retries().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_run_until_shutdown_continues_ticking_on_tick_errors_with_backoff() {
    let offchain = Arc::new(ConfigurableOffChainStore::new(vec![1], HashMap::new()));
    let onchain = Arc::new(ConfigurableOnChainAdapter::new(HashMap::new()));
    let alerts = Arc::new(TestAlertSink::default());

    // Simulate complete tick failure at the store level
    offchain.fail_reconcilable_periods.store(true, Ordering::SeqCst);

    let backoff_config = BackoffConfig {
        min_backoff: Duration::from_millis(5),
        max_backoff: Duration::from_millis(20),
        backoff_factor: 1.5,
    };

    let job = Arc::new(
        ReconciliationJob::new(AgreementSpec, offchain.clone(), onchain, alerts.clone())
            .with_backoff_config(backoff_config),
    );

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let job_clone = job.clone();
    let handle = tokio::spawn(async move {
        job_clone
            .run_until_shutdown(Duration::from_millis(10), shutdown_rx)
            .await;
    });

    // Let the loop run through several failing ticks
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Verify task is still running (didn't exit or crash on error)
    assert!(!handle.is_finished());

    let events = alerts.events.lock().await;
    assert!(events.iter().any(|e| e.kind == AlertKind::ReconciliationTickFailed));

    // Send shutdown signal
    shutdown_tx.send(true).unwrap();

    // Verify clean shutdown
    tokio::time::timeout(Duration::from_millis(500), handle)
        .await
        .expect("should terminate cleanly on shutdown signal")
        .unwrap();
}

#[tokio::test]
async fn test_backlog_batch_bounding_prevents_stampede() {
    let mut aggregates = HashMap::new();
    let mut snapshots = HashMap::new();
    let mut periods = Vec::new();

    for i in 1..=10 {
        periods.push(i);
        aggregates.insert(
            i,
            OffChainAggregate {
                period: i,
                snapshot_hash: [i as u8; 32],
                source_data_hash: [i as u8; 32],
            },
        );
        snapshots.insert(
            i,
            OnChainSnapshotView {
                period: i,
                snapshot_hash: [i as u8; 32],
                source_data_hash: [i as u8; 32],
            },
        );
    }

    let offchain = Arc::new(ConfigurableOffChainStore::new(periods, aggregates));
    let onchain = Arc::new(ConfigurableOnChainAdapter::new(snapshots));
    let alerts = Arc::new(TestAlertSink::default());

    let job = ReconciliationJob::new(AgreementSpec, offchain, onchain, alerts)
        .with_max_batch_size(4);

    let report = job.run_once().await.expect("run_once should succeed");
    // Only 4 periods should be checked in this batch due to bounding limit
    assert_eq!(report.checked_periods, 4);
    assert_eq!(report.clean_periods().len(), 4);
}

#[tokio::test]
async fn test_simulated_outage_then_recovery_catches_up_without_duplicate_alerts() {
    let offchain = Arc::new(ConfigurableOffChainStore::new(
        vec![101, 102],
        HashMap::from([
            (
                101,
                OffChainAggregate {
                    period: 101,
                    snapshot_hash: [101; 32],
                    source_data_hash: [101; 32],
                },
            ),
            (
                102,
                OffChainAggregate {
                    period: 102,
                    snapshot_hash: [102; 32],
                    source_data_hash: [102; 32],
                },
            ),
        ]),
    ));

    let onchain = Arc::new(ConfigurableOnChainAdapter::new(HashMap::from([
        (
            101,
            OnChainSnapshotView {
                period: 101,
                snapshot_hash: [101; 32],
                source_data_hash: [101; 32],
            },
        ),
        (
            102,
            OnChainSnapshotView {
                period: 102,
                snapshot_hash: [102; 32],
                source_data_hash: [102; 32],
            },
        ),
    ])));

    let alerts = Arc::new(TestAlertSink::default());
    let job = ReconciliationJob::new(AgreementSpec, offchain.clone(), onchain.clone(), alerts.clone());

    // Outage starts: Both periods fail on on-chain read
    onchain.fail_periods.lock().await.insert(101);
    onchain.fail_periods.lock().await.insert(102);

    let report1 = job.run_once().await.unwrap();
    assert_eq!(report1.failed_periods().len(), 2);
    assert_eq!(alerts.events.lock().await.len(), 2);

    // Outage recovers
    onchain.fail_periods.lock().await.clear();

    // Next tick: catches up on both periods cleanly
    let report2 = job.run_once().await.unwrap();
    assert_eq!(report2.clean_periods().len(), 2);
    assert_eq!(report2.failed_periods().len(), 0);
    assert_eq!(report2.discrepant_periods().len(), 0);

    // No new failure or discrepancy alerts emitted on catch-up
    assert_eq!(alerts.events.lock().await.len(), 2);
}
