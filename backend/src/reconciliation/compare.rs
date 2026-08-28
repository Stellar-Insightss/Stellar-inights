use std::sync::Arc;
use std::time::{Duration, SystemTime};

use log::warn;
use prometheus::{register_int_counter, IntCounter};

use crate::reconciliation::{
    AlertEvent, AlertKind, AlertSeverity, AlertSink, AgreementSpec, Discrepancy,
    MissingSubmissionHandler, OffChainAggregateStore, OnChainSnapshotReader, ReconciliationError,
    ResubmissionReport,
};

lazy_static::lazy_static! {
    static ref RECONCILIATION_RUNS_TOTAL: IntCounter = register_int_counter!(
        "reconciliation_runs_total",
        "Total number of reconciliation runs"
    ).unwrap();

    static ref RECONCILIATION_DISCREPANCIES_TOTAL: IntCounter = register_int_counter!(
        "reconciliation_discrepancies_total",
        "Total number of reconciliation discrepancies detected"
    ).unwrap();
}

#[derive(Debug, Clone)]
pub struct ReconciliationReport {
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub checked_periods: usize,
    pub discrepancies: Vec<Discrepancy>,
    pub resubmission_report: Option<ResubmissionReport>,
}

pub struct ReconciliationJob {
    spec: AgreementSpec,
    offchain: Arc<dyn OffChainAggregateStore>,
    onchain: Arc<dyn OnChainSnapshotReader>,
    alerts: Arc<dyn AlertSink>,
    resubmitter: Option<MissingSubmissionHandler>,
}

impl ReconciliationJob {
    pub fn new(
        spec: AgreementSpec,
        offchain: Arc<dyn OffChainAggregateStore>,
        onchain: Arc<dyn OnChainSnapshotReader>,
        alerts: Arc<dyn AlertSink>,
    ) -> Self {
        Self {
            spec,
            offchain,
            onchain,
            alerts,
            resubmitter: None,
        }
    }

    pub fn with_resubmitter(mut self, handler: MissingSubmissionHandler) -> Self {
        self.resubmitter = Some(handler);
        self
    }

    pub async fn run_once(&self) -> Result<ReconciliationReport, ReconciliationError> {
        RECONCILIATION_RUNS_TOTAL.inc();
        let started_at = SystemTime::now();

        let resubmission_report = if let Some(handler) = &self.resubmitter {
            Some(handler.run_once().await?)
        } else {
            None
        };

        let periods = self.offchain.reconcilable_periods().await?;
        let mut discrepancies = Vec::new();

        for period in &periods {
            let offchain = self.offchain.get_aggregate(*period).await?;
            let onchain = self.onchain.get_snapshot(*period).await?;
            let period_discrepancies = self
                .spec
                .compare(*period, offchain.as_ref(), onchain.as_ref());

            for discrepancy in period_discrepancies {
                RECONCILIATION_DISCREPANCIES_TOTAL.inc();

                if self.spec.is_above_tolerance(&discrepancy) {
                    warn!(
                        "reconciliation discrepancy for period {}: {:?}",
                        discrepancy.period, discrepancy.kind
                    );
                    self.alerts
                        .emit(AlertEvent {
                            kind: AlertKind::DiscrepancyDetected,
                            severity: AlertSeverity::Warning,
                            period: discrepancy.period,
                            message: discrepancy.detail.clone(),
                        })
                        .await?;
                }

                discrepancies.push(discrepancy);
            }
        }

        Ok(ReconciliationReport {
            started_at,
            finished_at: SystemTime::now(),
            checked_periods: periods.len(),
            discrepancies,
            resubmission_report,
        })
    }

    pub async fn run_forever(&self, interval: Duration) -> Result<(), ReconciliationError> {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            self.run_once().await?;
        }
    }
}
