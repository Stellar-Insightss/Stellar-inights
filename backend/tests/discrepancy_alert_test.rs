use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use stellar_insights_backend::reconciliation::{
    AlertEvent, AlertKind, AlertSink, AgreementSpec, MissingSubmissionHandler,
    OffChainAggregate, OffChainAggregateStore, OnChainSnapshotReader, OnChainSnapshotView,
    OnChainSnapshotWriter, ReconciliationError, ReconciliationJob,
};

struct InMemoryOffChainStore {
    periods: Vec<u64>,
    aggregates: HashMap<u64, OffChainAggregate>,
}

#[async_trait]
impl OffChainAggregateStore for InMemoryOffChainStore {
    async fn reconcilable_periods(&self) -> Result<Vec<u64>, ReconciliationError> {
        Ok(self.periods.clone())
    }

    async fn get_aggregate(
        &self,
        period: u64,
    ) -> Result<Option<OffChainAggregate>, ReconciliationError> {
        Ok(self.aggregates.get(&period).cloned())
    }
}

struct InMemoryOnChainAdapter {
    snapshots: Arc<Mutex<HashMap<u64, OnChainSnapshotView>>>,
    fail_submit_for: HashSet<u64>,
}

#[async_trait]
impl OnChainSnapshotReader for InMemoryOnChainAdapter {
    async fn get_snapshot(
        &self,
        period: u64,
    ) -> Result<Option<OnChainSnapshotView>, ReconciliationError> {
        Ok(self.snapshots.lock().await.get(&period).cloned())
    }
}

#[async_trait]
impl OnChainSnapshotWriter for InMemoryOnChainAdapter {
    async fn submit_snapshot(
        &self,
        aggregate: &OffChainAggregate,
    ) -> Result<(), ReconciliationError> {
        if self.fail_submit_for.contains(&aggregate.period) {
            return Err(ReconciliationError::OnChainWrite(
                "simulated submit failure".to_string(),
            ));
        }

        self.snapshots.lock().await.insert(
            aggregate.period,
            OnChainSnapshotView {
                period: aggregate.period,
                snapshot_hash: aggregate.snapshot_hash,
                source_data_hash: aggregate.source_data_hash,
            },
        );
        Ok(())
    }
}

#[derive(Default)]
struct RecordingAlertSink {
    events: Arc<Mutex<Vec<AlertEvent>>>,
}

#[async_trait]
impl AlertSink for RecordingAlertSink {
    async fn emit(&self, event: AlertEvent) -> Result<(), ReconciliationError> {
        self.events.lock().await.push(event);
        Ok(())
    }
}

#[tokio::test]
async fn discrepancy_above_tolerance_emits_alert() {
    let offchain = Arc::new(InMemoryOffChainStore {
        periods: vec![1],
        aggregates: HashMap::from([(
            1,
            OffChainAggregate {
                period: 1,
                snapshot_hash: [1; 32],
                source_data_hash: [2; 32],
            },
        )]),
    });

    let onchain = Arc::new(InMemoryOnChainAdapter {
        snapshots: Arc::new(Mutex::new(HashMap::from([(
            1,
            OnChainSnapshotView {
                period: 1,
                snapshot_hash: [9; 32],
                source_data_hash: [2; 32],
            },
        )]))),
        fail_submit_for: HashSet::new(),
    });

    let alerts = Arc::new(RecordingAlertSink::default());

    let job = ReconciliationJob::new(AgreementSpec, offchain, onchain, alerts.clone());
    let report = job.run_once().await.expect("reconciliation should run");

    assert_eq!(report.discrepancies.len(), 1);

    let events = alerts.events.lock().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, AlertKind::DiscrepancyDetected);
    assert_eq!(events[0].period, 1);
}

#[tokio::test]
async fn missing_snapshot_is_auto_resubmitted() {
    let aggregate = OffChainAggregate {
        period: 3,
        snapshot_hash: [7; 32],
        source_data_hash: [8; 32],
    };

    let offchain = Arc::new(InMemoryOffChainStore {
        periods: vec![3],
        aggregates: HashMap::from([(3, aggregate.clone())]),
    });

    let onchain = Arc::new(InMemoryOnChainAdapter {
        snapshots: Arc::new(Mutex::new(HashMap::new())),
        fail_submit_for: HashSet::new(),
    });

    let alerts = Arc::new(RecordingAlertSink::default());

    let resubmitter = MissingSubmissionHandler::new(
        offchain.clone(),
        onchain.clone(),
        onchain.clone(),
        alerts.clone(),
    );

    let job = ReconciliationJob::new(AgreementSpec, offchain, onchain.clone(), alerts.clone())
        .with_resubmitter(resubmitter);

    let report = job.run_once().await.expect("reconciliation should run");

    assert!(report.discrepancies.is_empty());
    let snapshot = onchain
        .get_snapshot(3)
        .await
        .expect("on-chain read should work");
    assert!(snapshot.is_some());

    let events = alerts.events.lock().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn failed_auto_resubmit_raises_operational_alert() {
    let offchain = Arc::new(InMemoryOffChainStore {
        periods: vec![7],
        aggregates: HashMap::from([(
            7,
            OffChainAggregate {
                period: 7,
                snapshot_hash: [5; 32],
                source_data_hash: [6; 32],
            },
        )]),
    });

    let onchain = Arc::new(InMemoryOnChainAdapter {
        snapshots: Arc::new(Mutex::new(HashMap::new())),
        fail_submit_for: HashSet::from([7]),
    });

    let alerts = Arc::new(RecordingAlertSink::default());

    let resubmitter = MissingSubmissionHandler::new(
        offchain.clone(),
        onchain.clone(),
        onchain.clone(),
        alerts.clone(),
    );

    let job = ReconciliationJob::new(AgreementSpec, offchain, onchain, alerts.clone())
        .with_resubmitter(resubmitter);

    let _report = job.run_once().await.expect("reconciliation should run");

    let events = alerts.events.lock().await;
    assert!(events
        .iter()
        .any(|event| event.kind == AlertKind::MissingSubmissionResubmitFailed));
}
