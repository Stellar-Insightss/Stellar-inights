//! Payment reliability and SLA tracking for cross-border transactions.
//!
//! Tracks outcome classifications (Success, Failed, TimedOut, Rejected) and SLA compliance,
//! providing mergeable aggregate statistics across windows and corridors.

use serde::{Deserialize, Serialize};

/// Payment transaction outcome status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaymentStatus {
    /// Payment settled successfully on ledger.
    Success,
    /// Payment failed (e.g. insufficient funds, path not found, bad auth).
    Failed,
    /// Payment exceeded max settlement timeout before inclusion.
    TimedOut,
    /// Payment rejected by entrypoint / pre-flight validation.
    Rejected,
}

/// Identifies a cross-border payment corridor (e.g., "USDC" -> "EURC").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaymentCorridor {
    pub source_asset: String,
    pub dest_asset: String,
}

impl PaymentCorridor {
    pub fn new(source_asset: impl Into<String>, dest_asset: impl Into<String>) -> Self {
        Self {
            source_asset: source_asset.into(),
            dest_asset: dest_asset.into(),
        }
    }
}

/// Mergeable reliability counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReliabilityCounters {
    pub total_payments: u64,
    pub successful_payments: u64,
    pub failed_payments: u64,
    pub timed_out_payments: u64,
    pub rejected_payments: u64,
    pub sla_breach_count: u64,
}

/// Computed reliability summary metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReliabilitySummary {
    pub total_payments: u64,
    pub successful_payments: u64,
    pub failed_payments: u64,
    pub timed_out_payments: u64,
    pub rejected_payments: u64,
    pub sla_breach_count: u64,
    /// Fraction of payments that succeeded ($N_{\text{success}} / N_{\text{total}}$).
    pub success_rate: f64,
    /// Fraction of payments that failed ($N_{\text{failed}} / N_{\text{total}}$).
    pub failure_rate: f64,
    /// Fraction of payments that timed out ($N_{\text{timeout}} / N_{\text{total}}$).
    pub timeout_rate: f64,
    /// Fraction of payments meeting latency SLA ($1.0 - \text{SLA Breaches} / N_{\text{total}}$).
    pub sla_compliance_rate: f64,
    /// High-availability metric (e.g. 99.95%).
    pub availability_percent: f64,
}

impl ReliabilityCounters {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a payment outcome and whether it breached the latency SLA.
    pub fn record(&mut self, status: PaymentStatus, sla_breached: bool) {
        self.total_payments += 1;
        match status {
            PaymentStatus::Success => self.successful_payments += 1,
            PaymentStatus::Failed => self.failed_payments += 1,
            PaymentStatus::TimedOut => self.timed_out_payments += 1,
            PaymentStatus::Rejected => self.rejected_payments += 1,
        }
        if sla_breached {
            self.sla_breach_count += 1;
        }
    }

    /// Merges counters from another shard or window ($C = C_1 \oplus C_2$).
    pub fn merge(&mut self, other: &Self) {
        self.total_payments += other.total_payments;
        self.successful_payments += other.successful_payments;
        self.failed_payments += other.failed_payments;
        self.timed_out_payments += other.timed_out_payments;
        self.rejected_payments += other.rejected_payments;
        self.sla_breach_count += other.sla_breach_count;
    }

    /// Computes summary ratios and availability percentages.
    pub fn summary(&self) -> ReliabilitySummary {
        if self.total_payments == 0 {
            return ReliabilitySummary {
                total_payments: 0,
                successful_payments: 0,
                failed_payments: 0,
                timed_out_payments: 0,
                rejected_payments: 0,
                sla_breach_count: 0,
                success_rate: 1.0,
                failure_rate: 0.0,
                timeout_rate: 0.0,
                sla_compliance_rate: 1.0,
                availability_percent: 100.0,
            };
        }

        let total = self.total_payments as f64;
        let success_rate = (self.successful_payments as f64) / total;
        let failure_rate = (self.failed_payments as f64) / total;
        let timeout_rate = (self.timed_out_payments as f64) / total;
        let sla_compliance_rate = 1.0 - ((self.sla_breach_count as f64) / total).min(1.0);
        let availability_percent = success_rate * 100.0;

        ReliabilitySummary {
            total_payments: self.total_payments,
            successful_payments: self.successful_payments,
            failed_payments: self.failed_payments,
            timed_out_payments: self.timed_out_payments,
            rejected_payments: self.rejected_payments,
            sla_breach_count: self.sla_breach_count,
            success_rate,
            failure_rate,
            timeout_rate,
            sla_compliance_rate,
            availability_percent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reliability_empty() {
        let counters = ReliabilityCounters::new();
        let summary = counters.summary();
        assert_eq!(summary.total_payments, 0);
        assert_eq!(summary.success_rate, 1.0);
        assert_eq!(summary.availability_percent, 100.0);
    }

    #[test]
    fn test_reliability_recording() {
        let mut counters = ReliabilityCounters::new();
        counters.record(PaymentStatus::Success, false);
        counters.record(PaymentStatus::Success, true); // Succeeded but breached SLA
        counters.record(PaymentStatus::Failed, true);
        counters.record(PaymentStatus::TimedOut, true);

        let summary = counters.summary();
        assert_eq!(summary.total_payments, 4);
        assert_eq!(summary.successful_payments, 2);
        assert_eq!(summary.failed_payments, 1);
        assert_eq!(summary.timed_out_payments, 1);
        assert_eq!(summary.sla_breach_count, 3);
        assert_eq!(summary.success_rate, 0.5);
        assert_eq!(summary.failure_rate, 0.25);
        assert_eq!(summary.timeout_rate, 0.25);
        assert_eq!(summary.sla_compliance_rate, 0.25);
    }

    #[test]
    fn test_reliability_merge() {
        let mut c1 = ReliabilityCounters::new();
        let mut c2 = ReliabilityCounters::new();

        c1.record(PaymentStatus::Success, false);
        c2.record(PaymentStatus::Failed, true);

        c1.merge(&c2);
        assert_eq!(c1.total_payments, 2);
        assert_eq!(c1.successful_payments, 1);
        assert_eq!(c1.failed_payments, 1);
        assert_eq!(c1.sla_breach_count, 1);
    }
}
