//! Two clocks, one truth: Clock reconciliation and Latency Clock Basis.
//!
//! # Clock Architecture
//!
//! Stellar payment analytics fundamentally operates across two distinct clock domains:
//!
//! 1. **Event-Time Domain ($T_{\text{ledger}}$)**:
//!    - The authoritative, deterministic consensus timestamp recorded in the Stellar ledger header.
//!    - **Role**: All time-windowing, watermarking, historical replays, and reliability/SLA metrics
//!      MUST strictly use $T_{\text{ledger}}$ as the time basis. This guarantees that analytics results
//!      are 100% deterministic and reproducible across node restarts, backlog re-indexing, and shard merges.
//!
//! 2. **Processing-Time Domain ($T_{\text{ingest}}$)**:
//!    - Local system wall-clock time when an event is received and processed by the indexer.
//!    - **Role**: Used exclusively for pipeline health observability, ingestion lag monitoring
//!      ($T_{\text{ingest}} - T_{\text{ledger}}$), and detecting upstream RPC replication backpressure.
//!
//! # Latency Clock Basis
//!
//! Cross-border payment latency is categorized into three explicit metrics:
//!
//! - **Settlement Latency ($L_{\text{settle}}$)**:
//!   $$L_{\text{settle}} = T_{\text{ledger}} - T_{\text{client\_submitted}}$$
//!   Measures the true on-chain settlement duration experienced by users. If $T_{\text{client\_submitted}}$
//!   is unavailable, the event's recorded transaction execution latency or ledger interval is used.
//!
//! - **Ingestion Lag ($L_{\text{ingest}}$)**:
//!   $$L_{\text{ingest}} = T_{\text{ingest}} - T_{\text{ledger}}$$
//!   Measures indexer delay and Horizon/RPC propagation lag.
//!
//! - **End-to-End Latency ($L_{\text{e2e}}$)**:
//!   $$L_{\text{e2e}} = T_{\text{ingest}} - T_{\text{client\_submitted}}$$
//!   Total latency from client submission to backend indexing.
//!
//! # Clock Skew Handling
//!
//! When event timestamps arrive in the future relative to wall-clock time ($T_{\text{ledger}} > T_{\text{ingest}} + \Delta_{\text{skew}}$),
//! a clock skew incident is flagged and recorded without dropping data or halting pipeline execution.

use crate::analytics::reliability::{PaymentCorridor, PaymentStatus};
use serde::{Deserialize, Serialize};

/// Maximum tolerable clock skew before generating an alert (60 seconds).
pub const DEFAULT_MAX_CLOCK_SKEW_SECS: u64 = 60;

/// Default SLA latency threshold (5,000 milliseconds / 5 seconds).
pub const DEFAULT_SLA_THRESHOLD_MS: f64 = 5_000.0;

/// Clock domain selector for queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockDomain {
    /// Ledger consensus timestamp (Event time).
    EventTime,
    /// Host wall-clock timestamp (Processing time).
    ProcessingTime,
}

/// Payment event ingested into the analytics engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentEvent {
    /// Unique payment or transaction hash.
    pub payment_id: String,
    /// Stellar ledger sequence number.
    pub ledger_sequence: u64,
    /// Deterministic ledger close time (Unix timestamp in seconds).
    pub ledger_closed_at: u64,
    /// Client submission time (Unix timestamp in seconds), if available.
    pub client_submitted_at: Option<u64>,
    /// Indexer wall-clock ingestion time (Unix timestamp in seconds).
    pub ingested_at: u64,
    /// Payment outcome.
    pub status: PaymentStatus,
    /// Optional payment corridor (asset pair).
    pub corridor: Option<PaymentCorridor>,
    /// Explicitly measured execution/settlement latency in milliseconds.
    pub latency_ms: Option<f64>,
}

impl PaymentEvent {
    /// Returns the authoritative event timestamp in seconds ($T_{\text{ledger}}$).
    pub fn event_time(&self) -> u64 {
        self.ledger_closed_at
    }

    /// Computes or retrieves the settlement latency in milliseconds.
    pub fn effective_latency_ms(&self, default_sla_threshold: f64) -> f64 {
        if let Some(lat) = self.latency_ms {
            return lat.max(0.0);
        }

        if let Some(submitted) = self.client_submitted_at {
            if self.ledger_closed_at >= submitted {
                let diff_secs = self.ledger_closed_at - submitted;
                return (diff_secs as f64) * 1000.0;
            }
        }

        // If timed out or failed without explicit latency, use default SLA threshold
        if self.status == PaymentStatus::TimedOut {
            default_sla_threshold * 1.5
        } else {
            0.0
        }
    }

    /// Computes the ingestion lag ($T_{\text{ingest}} - T_{\text{ledger}}$) in seconds.
    pub fn ingestion_lag_secs(&self) -> i64 {
        (self.ingested_at as i64) - (self.ledger_closed_at as i64)
    }

    /// Determines if there is significant clock skew (ledger time > ingest time + max skew).
    pub fn is_clock_skewed(&self, max_skew_secs: u64) -> bool {
        self.ledger_closed_at > self.ingested_at + max_skew_secs
    }

    /// Checks if this payment breached the specified latency SLA threshold.
    pub fn is_sla_breached(&self, sla_threshold_ms: f64) -> bool {
        if self.status == PaymentStatus::TimedOut {
            return true;
        }
        self.effective_latency_ms(sla_threshold_ms) > sla_threshold_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_event_latency_calculation() {
        let event = PaymentEvent {
            payment_id: "tx_123".into(),
            ledger_sequence: 100,
            ledger_closed_at: 1700000005,
            client_submitted_at: Some(1700000000),
            ingested_at: 1700000007,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: None,
        };

        assert_eq!(event.event_time(), 1700000005);
        assert_eq!(event.effective_latency_ms(5000.0), 5000.0);
        assert_eq!(event.ingestion_lag_secs(), 2);
        assert!(!event.is_clock_skewed(60));
        assert!(!event.is_sla_breached(5000.0));
    }

    #[test]
    fn test_clock_skew_detection() {
        let skewed_event = PaymentEvent {
            payment_id: "tx_skew".into(),
            ledger_sequence: 100,
            ledger_closed_at: 1700000200, // 200s in the future
            client_submitted_at: None,
            ingested_at: 1700000000,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(150.0),
        };

        assert!(skewed_event.is_clock_skewed(60));
        assert_eq!(skewed_event.effective_latency_ms(5000.0), 150.0);
    }
}
