//! Prometheus metrics for TTL monitoring and rent-bumping.
//!
//! All metrics are registered once via [`TtlMetrics::register`] and
//! updated by the [`TtlManager`](super::ttl_manager::TtlManager) poll loop.
//!
//! # Exposed metrics
//!
//! | Metric | Type | Description |
//! |---|---|---|
//! | `contract_ttl_remaining_ledgers` | Gauge | Current remaining TTL in ledgers per contract |
//! | `contract_ttl_bump_total` | Counter | Total number of successful bump transactions per contract |
//! | `contract_ttl_bump_errors_total` | Counter | Total number of failed bump attempts per contract |
//! | `contract_ttl_bump_skipped_total` | Counter | Bumps skipped because TTL was still above threshold |
//! | `contract_ttl_last_bump_timestamp_seconds` | Gauge | Unix timestamp of the last successful bump per contract |
//! | `contract_ttl_check_duration_seconds` | Histogram | Duration of each TTL check cycle |

use prometheus::{
    GaugeVec, Histogram, HistogramOpts, IntCounterVec, Opts, Registry,
};

/// Centralised handle to all TTL-related Prometheus metrics.
///
/// Cheaply cloneable — all internals are wrapped in `Arc` by prometheus.
#[derive(Clone)]
pub struct TtlMetrics {
    /// Remaining ledgers for each contract's instance + persistent storage.
    pub remaining_ledgers: GaugeVec,
    /// Cumulative successful bumps per contract.
    pub bump_success: IntCounterVec,
    /// Cumulative failed bump attempts per contract.
    pub bump_errors: IntCounterVec,
    /// Bumps skipped (TTL still healthy).
    pub bump_skipped: IntCounterVec,
    /// Unix timestamp of the last successful bump per contract.
    pub last_bump_timestamp: GaugeVec,
    /// Wall-clock duration of each full check cycle.
    pub check_duration: Histogram,
}

impl TtlMetrics {
    /// Create and register all metrics with the given Prometheus registry.
    pub fn register(registry: &Registry) -> Result<Self, prometheus::Error> {
        let remaining_ledgers = GaugeVec::new(
            Opts::new(
                "contract_ttl_remaining_ledgers",
                "Current remaining TTL in ledgers for a contract",
            ),
            &["contract"],
        )?;
        registry.register(Box::new(remaining_ledgers.clone()))?;

        let bump_success = IntCounterVec::new(
            Opts::new(
                "contract_ttl_bump_total",
                "Total successful TTL bump transactions",
            ),
            &["contract"],
        )?;
        registry.register(Box::new(bump_success.clone()))?;

        let bump_errors = IntCounterVec::new(
            Opts::new(
                "contract_ttl_bump_errors_total",
                "Total failed TTL bump attempts",
            ),
            &["contract"],
        )?;
        registry.register(Box::new(bump_errors.clone()))?;

        let bump_skipped = IntCounterVec::new(
            Opts::new(
                "contract_ttl_bump_skipped_total",
                "Bumps skipped because TTL was still above threshold",
            ),
            &["contract"],
        )?;
        registry.register(Box::new(bump_skipped.clone()))?;

        let last_bump_timestamp = GaugeVec::new(
            Opts::new(
                "contract_ttl_last_bump_timestamp_seconds",
                "Unix timestamp of the last successful TTL bump",
            ),
            &["contract"],
        )?;
        registry.register(Box::new(last_bump_timestamp.clone()))?;

        let check_duration = Histogram::with_opts(
            HistogramOpts::new(
                "contract_ttl_check_duration_seconds",
                "Duration of each TTL check cycle",
            )
            .buckets(vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        )?;
        registry.register(Box::new(check_duration.clone()))?;

        Ok(Self {
            remaining_ledgers,
            bump_success,
            bump_errors,
            bump_skipped,
            last_bump_timestamp,
            check_duration,
        })
    }

    /// Record the remaining TTL for a contract.
    pub fn set_remaining(&self, contract: &str, ledgers: u32) {
        self.remaining_ledgers
            .with_label_values(&[contract])
            .set(f64::from(ledgers));
    }

    /// Record a successful bump.
    pub fn record_bump_success(&self, contract: &str) {
        self.bump_success.with_label_values(&[contract]).inc();
        self.last_bump_timestamp
            .with_label_values(&[contract])
            .set(chrono::Utc::now().timestamp() as f64);
    }

    /// Record a failed bump attempt.
    pub fn record_bump_error(&self, contract: &str) {
        self.bump_errors.with_label_values(&[contract]).inc();
    }

    /// Record that a bump was skipped (TTL healthy).
    pub fn record_bump_skipped(&self, contract: &str) {
        self.bump_skipped.with_label_values(&[contract]).inc();
    }

    /// Record the duration of a full check cycle.
    pub fn observe_check_duration(&self, seconds: f64) {
        self.check_duration.observe(seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_record_metrics() {
        let registry = Registry::new();
        let metrics = TtlMetrics::register(&registry).unwrap();

        metrics.set_remaining("analytics", 400_000);
        metrics.record_bump_success("analytics");
        metrics.record_bump_skipped("escrow");
        metrics.record_bump_error("multisig");
        metrics.observe_check_duration(0.42);

        // Verify metrics were registered (gather should not fail).
        let families = registry.gather();
        assert!(!families.is_empty());

        // Spot-check remaining_ledgers gauge.
        let remaining = families
            .iter()
            .find(|f| f.get_name() == "contract_ttl_remaining_ledgers")
            .expect("remaining_ledgers metric not found");
        assert_eq!(remaining.get_metric()[0].get_label()[0].get_value(), "analytics");
    }

    #[test]
    fn metrics_are_cloneable() {
        let registry = Registry::new();
        let m1 = TtlMetrics::register(&registry).unwrap();
        let m2 = m1.clone();
        m1.set_remaining("test", 100);
        m2.record_bump_success("test");
    }
}
