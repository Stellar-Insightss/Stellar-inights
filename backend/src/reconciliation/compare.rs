use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use log::{error, info, warn};
use prometheus::{register_int_counter, IntCounter};
use tokio::sync::Mutex;

use crate::reconciliation::{
    AgreementSpec, AlertEvent, AlertKind, AlertSeverity, AlertSink, Discrepancy,
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

    static ref RECONCILIATION_FAILURES_TOTAL: IntCounter = register_int_counter!(
        "reconciliation_failures_total",
        "Total number of reconciliation period fetch failures"
    ).unwrap();

    static ref RECONCILIATION_TICK_ERRORS_TOTAL: IntCounter = register_int_counter!(
        "reconciliation_tick_errors_total",
        "Total number of critical reconciliation tick errors"
    ).unwrap();
}

/// Status of reconciliation for an individual period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeriodReconciliationStatus {
    Clean,
    Discrepant(Vec<Discrepancy>),
    Failed(String),
}

/// Per-period reconciliation outcome in a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeriodOutcome {
    pub period: u64,
    pub status: PeriodReconciliationStatus,
}

/// Durable or in-memory state tracking for retry and watermark backlog.
#[async_trait]
pub trait ReconciliationStateStore: Send + Sync {
    /// Load all periods that previously failed to reconcile and need retry.
    async fn load_pending_retries(&self) -> Result<Vec<u64>, ReconciliationError>;

    /// Record a period that failed to reconcile (e.g. transient RPC/store error).
    async fn record_failed_period(
        &self,
        period: u64,
        error: &str,
    ) -> Result<(), ReconciliationError>;

    /// Record a period that was successfully reconciled (clean or with detected discrepancy).
    async fn record_reconciled_period(&self, period: u64) -> Result<(), ReconciliationError>;
}

/// Thread-safe in-memory state store implementing `ReconciliationStateStore`.
#[derive(Debug, Default, Clone)]
pub struct InMemoryReconciliationStore {
    failed_periods: Arc<Mutex<BTreeMap<u64, String>>>,
    reconciled_periods: Arc<Mutex<BTreeSet<u64>>>,
}

impl InMemoryReconciliationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn failed_periods_count(&self) -> usize {
        self.failed_periods.lock().await.len()
    }

    pub async fn is_reconciled(&self, period: u64) -> bool {
        self.reconciled_periods.lock().await.contains(&period)
    }

    pub async fn reconciled_periods_count(&self) -> usize {
        self.reconciled_periods.lock().await.len()
    }
}

#[async_trait]
impl ReconciliationStateStore for InMemoryReconciliationStore {
    async fn load_pending_retries(&self) -> Result<Vec<u64>, ReconciliationError> {
        let failed = self.failed_periods.lock().await;
        Ok(failed.keys().cloned().collect())
    }

    async fn record_failed_period(
        &self,
        period: u64,
        error: &str,
    ) -> Result<(), ReconciliationError> {
        let mut failed = self.failed_periods.lock().await;
        failed.insert(period, error.to_string());
        let mut reconciled = self.reconciled_periods.lock().await;
        reconciled.remove(&period);
        Ok(())
    }

    async fn record_reconciled_period(&self, period: u64) -> Result<(), ReconciliationError> {
        let mut failed = self.failed_periods.lock().await;
        failed.remove(&period);
        let mut reconciled = self.reconciled_periods.lock().await;
        reconciled.insert(period);
        Ok(())
    }
}

/// Configuration for exponential backoff during consecutive tick failures.
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    pub min_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_factor: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            min_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_factor: 2.0,
        }
    }
}

impl BackoffConfig {
    pub fn calculate_backoff(&self, consecutive_failures: u32) -> Duration {
        if consecutive_failures == 0 {
            return Duration::ZERO;
        }
        let factor = self
            .backoff_factor
            .powi((consecutive_failures - 1).min(10) as i32);
        let backoff_secs = self.min_backoff.as_secs_f64() * factor;
        let clamped = Duration::from_secs_f64(backoff_secs).min(self.max_backoff);
        clamped.max(self.min_backoff)
    }
}

#[derive(Debug, Clone)]
pub struct ReconciliationReport {
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub checked_periods: usize,
    pub discrepancies: Vec<Discrepancy>,
    pub period_outcomes: Vec<PeriodOutcome>,
    pub resubmission_report: Option<ResubmissionReport>,
}

impl ReconciliationReport {
    pub fn clean_periods(&self) -> Vec<u64> {
        self.period_outcomes
            .iter()
            .filter_map(|o| {
                if matches!(o.status, PeriodReconciliationStatus::Clean) {
                    Some(o.period)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn discrepant_periods(&self) -> Vec<u64> {
        self.period_outcomes
            .iter()
            .filter_map(|o| {
                if matches!(o.status, PeriodReconciliationStatus::Discrepant(_)) {
                    Some(o.period)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn failed_periods(&self) -> Vec<u64> {
        self.period_outcomes
            .iter()
            .filter_map(|o| {
                if matches!(o.status, PeriodReconciliationStatus::Failed(_)) {
                    Some(o.period)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn has_failures(&self) -> bool {
        self.period_outcomes
            .iter()
            .any(|o| matches!(o.status, PeriodReconciliationStatus::Failed(_)))
    }

    pub fn has_discrepancies(&self) -> bool {
        !self.discrepancies.is_empty()
    }
}

pub struct ReconciliationJob {
    spec: AgreementSpec,
    offchain: Arc<dyn OffChainAggregateStore>,
    onchain: Arc<dyn OnChainSnapshotReader>,
    alerts: Arc<dyn AlertSink>,
    resubmitter: Option<MissingSubmissionHandler>,
    state_store: Arc<dyn ReconciliationStateStore>,
    backoff_config: BackoffConfig,
    max_batch_size: Option<usize>,
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
            state_store: Arc::new(InMemoryReconciliationStore::new()),
            backoff_config: BackoffConfig::default(),
            max_batch_size: None,
        }
    }

    pub fn with_resubmitter(mut self, handler: MissingSubmissionHandler) -> Self {
        self.resubmitter = Some(handler);
        self
    }

    pub fn with_state_store(mut self, store: Arc<dyn ReconciliationStateStore>) -> Self {
        self.state_store = store;
        self
    }

    pub fn with_backoff_config(mut self, config: BackoffConfig) -> Self {
        self.backoff_config = config;
        self
    }

    pub fn with_max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = Some(size);
        self
    }

    pub fn state_store(&self) -> Arc<dyn ReconciliationStateStore> {
        self.state_store.clone()
    }

    pub fn backoff_config(&self) -> &BackoffConfig {
        &self.backoff_config
    }

    pub async fn run_once(&self) -> Result<ReconciliationReport, ReconciliationError> {
        RECONCILIATION_RUNS_TOTAL.inc();
        let started_at = SystemTime::now();

        // 1. Optional resubmission phase (isolated against transient failure)
        let resubmission_report = if let Some(handler) = &self.resubmitter {
            match handler.run_once().await {
                Ok(rep) => Some(rep),
                Err(err) => {
                    warn!("resubmission handler run failed: {}", err);
                    let _ = self
                        .alerts
                        .emit(AlertEvent {
                            kind: AlertKind::MissingSubmissionResubmitFailed,
                            severity: AlertSeverity::Warning,
                            period: 0,
                            message: format!("resubmission cycle error: {}", err),
                        })
                        .await;
                    None
                }
            }
        } else {
            None
        };

        // 2. Fetch fresh reconcilable periods from off-chain store
        let fresh_periods = self.offchain.reconcilable_periods().await?;

        // 3. Load pending retry backlog from state store
        let pending_retries = match self.state_store.load_pending_retries().await {
            Ok(retries) => retries,
            Err(err) => {
                warn!("failed loading pending retry backlog: {}", err);
                Vec::new()
            }
        };

        // 4. Merge fresh periods with retry backlog
        let mut period_set = BTreeSet::new();
        for period in fresh_periods {
            period_set.insert(period);
        }
        for period in pending_retries {
            period_set.insert(period);
        }

        let mut period_list: Vec<u64> = period_set.into_iter().collect();
        if let Some(max_batch) = self.max_batch_size {
            if period_list.len() > max_batch {
                period_list.truncate(max_batch);
            }
        }

        let mut discrepancies = Vec::new();
        let mut period_outcomes = Vec::with_capacity(period_list.len());

        for period in period_list {
            // Per-period offchain fetch with error isolation
            let offchain = match self.offchain.get_aggregate(period).await {
                Ok(agg) => agg,
                Err(err) => {
                    RECONCILIATION_FAILURES_TOTAL.inc();
                    warn!(
                        "failed fetching offchain aggregate for period {}: {}",
                        period, err
                    );
                    let err_msg = format!("offchain fetch error: {}", err);
                    let _ = self
                        .state_store
                        .record_failed_period(period, &err_msg)
                        .await;
                    let _ = self
                        .alerts
                        .emit(AlertEvent {
                            kind: AlertKind::ReconciliationPeriodFailed,
                            severity: AlertSeverity::Warning,
                            period,
                            message: err_msg.clone(),
                        })
                        .await;
                    period_outcomes.push(PeriodOutcome {
                        period,
                        status: PeriodReconciliationStatus::Failed(err_msg),
                    });
                    continue;
                }
            };

            // Per-period onchain fetch with error isolation
            let onchain = match self.onchain.get_snapshot(period).await {
                Ok(snap) => snap,
                Err(err) => {
                    RECONCILIATION_FAILURES_TOTAL.inc();
                    warn!(
                        "failed fetching onchain snapshot for period {}: {}",
                        period, err
                    );
                    let err_msg = format!("onchain fetch error: {}", err);
                    let _ = self
                        .state_store
                        .record_failed_period(period, &err_msg)
                        .await;
                    let _ = self
                        .alerts
                        .emit(AlertEvent {
                            kind: AlertKind::ReconciliationPeriodFailed,
                            severity: AlertSeverity::Warning,
                            period,
                            message: err_msg.clone(),
                        })
                        .await;
                    period_outcomes.push(PeriodOutcome {
                        period,
                        status: PeriodReconciliationStatus::Failed(err_msg),
                    });
                    continue;
                }
            };

            // Compare views via AgreementSpec
            let period_discrepancies = self
                .spec
                .compare(period, offchain.as_ref(), onchain.as_ref());

            if period_discrepancies.is_empty() {
                let _ = self.state_store.record_reconciled_period(period).await;
                period_outcomes.push(PeriodOutcome {
                    period,
                    status: PeriodReconciliationStatus::Clean,
                });
            } else {
                let _ = self.state_store.record_reconciled_period(period).await;
                for discrepancy in &period_discrepancies {
                    RECONCILIATION_DISCREPANCIES_TOTAL.inc();

                    if self.spec.is_above_tolerance(discrepancy) {
                        warn!(
                            "reconciliation discrepancy for period {}: {:?}",
                            discrepancy.period, discrepancy.kind
                        );
                        let _ = self
                            .alerts
                            .emit(AlertEvent {
                                kind: AlertKind::DiscrepancyDetected,
                                severity: AlertSeverity::Warning,
                                period: discrepancy.period,
                                message: discrepancy.detail.clone(),
                            })
                            .await;
                    }
                }
                discrepancies.extend(period_discrepancies.clone());
                period_outcomes.push(PeriodOutcome {
                    period,
                    status: PeriodReconciliationStatus::Discrepant(period_discrepancies),
                });
            }
        }

        let finished_at = SystemTime::now();
        let checked_periods = period_outcomes.len();

        Ok(ReconciliationReport {
            started_at,
            finished_at,
            checked_periods,
            discrepancies,
            period_outcomes,
            resubmission_report,
        })
    }

    /// Run the reconciliation loop indefinitely.
    ///
    /// Never propagates transient tick errors out of the loop. Applies exponential
    /// backoff on consecutive failures and continues ticking.
    pub async fn run_forever(&self, interval: Duration) -> Result<(), ReconciliationError> {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        self.run_until_shutdown(interval, rx).await;
        Ok(())
    }

    /// Run the reconciliation loop cooperatively until `shutdown` signal fires.
    pub async fn run_until_shutdown(
        &self,
        interval: Duration,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut consecutive_tick_failures: u32 = 0;

        info!(
            "ReconciliationJob daemon starting with tick interval {:?}",
            interval
        );

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match self.run_once().await {
                        Ok(report) => {
                            if report.has_failures() {
                                consecutive_tick_failures = consecutive_tick_failures.saturating_add(1);
                                let backoff = self.backoff_config.calculate_backoff(consecutive_tick_failures);
                                warn!(
                                    "Reconciliation tick completed with {} failed period(s); applying backoff {:?}",
                                    report.failed_periods().len(),
                                    backoff
                                );
                                if !backoff.is_zero() {
                                    tokio::select! {
                                        _ = tokio::time::sleep(backoff) => {}
                                        _ = shutdown.changed() => {
                                            if *shutdown.borrow() {
                                                info!("ReconciliationJob shutting down during backoff sleep");
                                                break;
                                            }
                                        }
                                    }
                                }
                            } else {
                                if consecutive_tick_failures > 0 {
                                    info!("Reconciliation recovered cleanly; resetting consecutive failure counter");
                                    consecutive_tick_failures = 0;
                                }
                            }
                        }
                        Err(err) => {
                            RECONCILIATION_TICK_ERRORS_TOTAL.inc();
                            consecutive_tick_failures = consecutive_tick_failures.saturating_add(1);
                            let backoff = self.backoff_config.calculate_backoff(consecutive_tick_failures);
                            error!(
                                "Reconciliation tick critical error: {}; consecutive failures: {}, backing off {:?}",
                                err, consecutive_tick_failures, backoff
                            );
                            let _ = self
                                .alerts
                                .emit(AlertEvent {
                                    kind: AlertKind::ReconciliationTickFailed,
                                    severity: AlertSeverity::Critical,
                                    period: 0,
                                    message: format!(
                                        "Reconciliation tick failed: {} (consecutive: {})",
                                        err, consecutive_tick_failures
                                    ),
                                })
                                .await;
                            if !backoff.is_zero() {
                                tokio::select! {
                                    _ = tokio::time::sleep(backoff) => {}
                                    _ = shutdown.changed() => {
                                        if *shutdown.borrow() {
                                            info!("ReconciliationJob shutting down during backoff sleep");
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("ReconciliationJob received shutdown signal; terminating daemon");
                        break;
                    }
                }
            }
        }
    }
}
