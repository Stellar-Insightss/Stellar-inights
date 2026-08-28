use std::sync::Arc;

use log::{info, warn};
use prometheus::{register_int_counter, IntCounter};

use crate::reconciliation::{
    AlertEvent, AlertKind, AlertSeverity, AlertSink, OffChainAggregateStore, OnChainSnapshotReader,
    OnChainSnapshotWriter, ReconciliationError,
};

lazy_static::lazy_static! {
    static ref RESUBMIT_ATTEMPTS_TOTAL: IntCounter = register_int_counter!(
        "reconciliation_resubmit_attempts_total",
        "Total number of missing on-chain resubmission attempts"
    ).unwrap();

    static ref RESUBMIT_FAILURES_TOTAL: IntCounter = register_int_counter!(
        "reconciliation_resubmit_failures_total",
        "Total number of missing on-chain resubmission failures"
    ).unwrap();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResubmissionStatus {
    Resubmitted,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResubmissionAttempt {
    pub period: u64,
    pub status: ResubmissionStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResubmissionReport {
    pub checked_periods: usize,
    pub missing_periods: usize,
    pub attempts: Vec<ResubmissionAttempt>,
}

pub struct MissingSubmissionHandler {
    offchain: Arc<dyn OffChainAggregateStore>,
    onchain_reader: Arc<dyn OnChainSnapshotReader>,
    onchain_writer: Arc<dyn OnChainSnapshotWriter>,
    alerts: Arc<dyn AlertSink>,
}

impl MissingSubmissionHandler {
    pub fn new(
        offchain: Arc<dyn OffChainAggregateStore>,
        onchain_reader: Arc<dyn OnChainSnapshotReader>,
        onchain_writer: Arc<dyn OnChainSnapshotWriter>,
        alerts: Arc<dyn AlertSink>,
    ) -> Self {
        Self {
            offchain,
            onchain_reader,
            onchain_writer,
            alerts,
        }
    }

    pub async fn run_once(&self) -> Result<ResubmissionReport, ReconciliationError> {
        let periods = self.offchain.reconcilable_periods().await?;
        let mut report = ResubmissionReport {
            checked_periods: periods.len(),
            ..Default::default()
        };

        for period in periods {
            let Some(aggregate) = self.offchain.get_aggregate(period).await? else {
                continue;
            };

            let onchain_snapshot = self.onchain_reader.get_snapshot(period).await?;
            if onchain_snapshot.is_some() {
                continue;
            }

            report.missing_periods += 1;
            RESUBMIT_ATTEMPTS_TOTAL.inc();

            match self.onchain_writer.submit_snapshot(&aggregate).await {
                Ok(()) => {
                    info!("resubmitted missing on-chain snapshot for period {}", period);
                    report.attempts.push(ResubmissionAttempt {
                        period,
                        status: ResubmissionStatus::Resubmitted,
                    });
                }
                Err(err) => {
                    RESUBMIT_FAILURES_TOTAL.inc();
                    warn!(
                        "failed resubmitting missing on-chain snapshot for period {}: {}",
                        period, err
                    );
                    self.alerts
                        .emit(AlertEvent {
                            kind: AlertKind::MissingSubmissionResubmitFailed,
                            severity: AlertSeverity::Critical,
                            period,
                            message: format!(
                                "automatic resubmission failed for missing on-chain snapshot: {}",
                                err
                            ),
                        })
                        .await?;

                    report.attempts.push(ResubmissionAttempt {
                        period,
                        status: ResubmissionStatus::Failed(err.to_string()),
                    });
                }
            }
        }

        Ok(report)
    }
}
