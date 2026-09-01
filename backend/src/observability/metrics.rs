//! Prometheus metrics for real-time payment reliability and latency percentile analytics.

use prometheus::{
    Gauge, GaugeVec, Histogram, HistogramOpts, IntCounter, IntCounterVec, Opts, Registry,
};

/// Handle to all payment analytics Prometheus metrics.
#[derive(Clone)]
pub struct PaymentAnalyticsMetrics {
    /// Total ingested payment events partitioned by status.
    pub payment_events_total: IntCounterVec,
    /// Total late-arriving events handled.
    pub late_events_total: IntCounter,
    /// Total clock skew anomalies detected.
    pub clock_skew_events_total: IntCounter,
    /// Current watermark timestamp (seconds).
    pub current_watermark: Gauge,
    /// Estimated P50 latency in milliseconds.
    pub p50_latency_ms: GaugeVec,
    /// Estimated P95 latency in milliseconds.
    pub p95_latency_ms: GaugeVec,
    /// Estimated P99 latency in milliseconds.
    pub p99_latency_ms: GaugeVec,
    /// Payment success rate (0.0 - 1.0).
    pub reliability_success_rate: GaugeVec,
    /// SLA compliance rate (0.0 - 1.0).
    pub sla_compliance_rate: GaugeVec,
    /// Ingestion lag between ledger close and indexer ingestion.
    pub ingestion_lag_seconds: Histogram,
}

impl PaymentAnalyticsMetrics {
    /// Registers payment analytics metrics with the provided Prometheus registry.
    pub fn register(registry: &Registry) -> Result<Self, prometheus::Error> {
        let payment_events_total = IntCounterVec::new(
            Opts::new(
                "payment_analytics_events_total",
                "Total number of payment events ingested",
            ),
            &["status"],
        )?;
        registry.register(Box::new(payment_events_total.clone()))?;

        let late_events_total = IntCounter::new(
            "payment_analytics_late_events_total",
            "Total number of late-arriving events handled after watermark finalization",
        )?;
        registry.register(Box::new(late_events_total.clone()))?;

        let clock_skew_events_total = IntCounter::new(
            "payment_analytics_clock_skew_events_total",
            "Total number of clock skew anomalies detected",
        )?;
        registry.register(Box::new(clock_skew_events_total.clone()))?;

        let current_watermark = Gauge::new(
            "payment_analytics_watermark_seconds",
            "Current watermark event timestamp in seconds",
        )?;
        registry.register(Box::new(current_watermark.clone()))?;

        let p50_latency_ms = GaugeVec::new(
            Opts::new(
                "payment_analytics_latency_p50_ms",
                "Estimated P50 payment latency in milliseconds",
            ),
            &["window_id"],
        )?;
        registry.register(Box::new(p50_latency_ms.clone()))?;

        let p95_latency_ms = GaugeVec::new(
            Opts::new(
                "payment_analytics_latency_p95_ms",
                "Estimated P95 payment latency in milliseconds",
            ),
            &["window_id"],
        )?;
        registry.register(Box::new(p95_latency_ms.clone()))?;

        let p99_latency_ms = GaugeVec::new(
            Opts::new(
                "payment_analytics_latency_p99_ms",
                "Estimated P99 payment latency in milliseconds",
            ),
            &["window_id"],
        )?;
        registry.register(Box::new(p99_latency_ms.clone()))?;

        let reliability_success_rate = GaugeVec::new(
            Opts::new(
                "payment_analytics_success_rate",
                "Payment success rate ratio (0.0 to 1.0)",
            ),
            &["window_id"],
        )?;
        registry.register(Box::new(reliability_success_rate.clone()))?;

        let sla_compliance_rate = GaugeVec::new(
            Opts::new(
                "payment_analytics_sla_compliance_rate",
                "Payment SLA compliance rate ratio (0.0 to 1.0)",
            ),
            &["window_id"],
        )?;
        registry.register(Box::new(sla_compliance_rate.clone()))?;

        let ingestion_lag_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "payment_analytics_ingestion_lag_seconds",
                "Ingestion lag between ledger consensus close and processing",
            )
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]),
        )?;
        registry.register(Box::new(ingestion_lag_seconds.clone()))?;

        Ok(Self {
            payment_events_total,
            late_events_total,
            clock_skew_events_total,
            current_watermark,
            p50_latency_ms,
            p95_latency_ms,
            p99_latency_ms,
            reliability_success_rate,
            sla_compliance_rate,
            ingestion_lag_seconds,
        })
    }
}

/// Global initialization helper for observability metrics.
pub fn init_metrics() -> Result<PaymentAnalyticsMetrics, prometheus::Error> {
    PaymentAnalyticsMetrics::register(prometheus::default_registry())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_analytics_metrics_registration() {
        let registry = Registry::new();
        let metrics = PaymentAnalyticsMetrics::register(&registry).expect("registration succeeds");

        metrics.payment_events_total.with_label_values(&["success"]).inc();
        metrics.late_events_total.inc();
        metrics.clock_skew_events_total.inc();
        metrics.current_watermark.set(1700000000.0);
        metrics.p50_latency_ms.with_label_values(&["w0"]).set(125.0);
        metrics.reliability_success_rate.with_label_values(&["w0"]).set(0.999);
        metrics.ingestion_lag_seconds.observe(1.2);

        let families = registry.gather();
        assert!(families.len() >= 7);
    }
}
