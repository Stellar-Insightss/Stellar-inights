pub mod compare;
pub mod resubmit;
pub mod spec;

use async_trait::async_trait;
use thiserror::Error;

pub use compare::{
    BackoffConfig, InMemoryReconciliationStore, PeriodOutcome, PeriodReconciliationStatus,
    ReconciliationJob, ReconciliationReport, ReconciliationStateStore,
};
pub use resubmit::{MissingSubmissionHandler, ResubmissionAttempt, ResubmissionReport, ResubmissionStatus};
pub use spec::{AgreementSpec, Discrepancy, DiscrepancyKind, OffChainAggregate, OnChainSnapshotView};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    DiscrepancyDetected,
    MissingSubmissionResubmitFailed,
    ReconciliationTickFailed,
    ReconciliationPeriodFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertEvent {
    pub kind: AlertKind,
    pub severity: AlertSeverity,
    pub period: u64,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum ReconciliationError {
    #[error("off-chain store failure: {0}")]
    OffChainStore(String),
    #[error("on-chain read failure: {0}")]
    OnChainRead(String),
    #[error("on-chain write failure: {0}")]
    OnChainWrite(String),
    #[error("alert sink failure: {0}")]
    AlertSink(String),
    #[error("state store failure: {0}")]
    StateStore(String),
}

#[async_trait]
pub trait OffChainAggregateStore: Send + Sync {
    async fn reconcilable_periods(&self) -> Result<Vec<u64>, ReconciliationError>;
    async fn get_aggregate(
        &self,
        period: u64,
    ) -> Result<Option<OffChainAggregate>, ReconciliationError>;
}

#[async_trait]
pub trait OnChainSnapshotReader: Send + Sync {
    async fn get_snapshot(
        &self,
        period: u64,
    ) -> Result<Option<OnChainSnapshotView>, ReconciliationError>;
}

#[async_trait]
pub trait OnChainSnapshotWriter: Send + Sync {
    async fn submit_snapshot(&self, aggregate: &OffChainAggregate)
        -> Result<(), ReconciliationError>;
}

#[async_trait]
pub trait AlertSink: Send + Sync {
    async fn emit(&self, event: AlertEvent) -> Result<(), ReconciliationError>;
}
