//! Watermarking engine and window lifecycle manager for out-of-order event streams.
//!
//! # Watermark Semantics & Out-of-Order Handling
//!
//! Stellar ledger events can arrive out-of-order due to distributed RPC replication lag, network retries,
//! or multi-node ingestion. To provide real-time latency percentiles without waiting indefinitely,
//! this module maintains a monotonically increasing watermark:
//!
//! $$W(t) = \max_{e \in \text{stream}}(T_{\text{ledger}}(e)) - \Delta_{\text{watermark}}$$
//!
//! ## Window Lifecycle
//!
//! 1. **`Active`**: A window $[T_{\text{start}}, T_{\text{end}})$ where $T_{\text{end}} > W(t)$.
//!    Events falling into this window are directly accumulated into live DDSketches and reliability counters.
//! 2. **`Finalized`**: A window where $T_{\text{end}} \le W(t)$. The watermark has passed the window boundary.
//!    The window is closed, sealed, and ready for on-chain reconciliation (`reconcilable_periods`).
//! 3. **`Amended`**: A finalized window that received late data under the `RetroactiveUpdate` policy.
//!
//! ## Late-Arriving Event Policies
//!
//! When an event arrives with $T_{\text{ledger}} < W(t)$ (belonging to an already finalized window):
//! - **`DropAndRecord`**: The late event is rejected from the finalized sketch to preserve deterministic
//!   finality, but is explicitly recorded in `late_events_count` and an audit log (never silently dropped).
//! - **`SideOutput`**: Routed to an isolated dead-letter/late-data queue for secondary reprocessing.
//! - **`RetroactiveUpdate`**: The finalized window's sketch is updated, its state transitions to `Amended`,
//!   and its revision counter is incremented to signal downstream consumers.

use crate::analytics::clock::{PaymentEvent, DEFAULT_MAX_CLOCK_SKEW_SECS, DEFAULT_SLA_THRESHOLD_MS};
use crate::analytics::reliability::{ReliabilityCounters, ReliabilitySummary};
use crate::analytics::sketch::{DDSketch, PercentileSummary, DEFAULT_ALPHA};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Default window size: 60 seconds.
pub const DEFAULT_WINDOW_SIZE_SECS: u64 = 60;

/// Default watermark delay (out-of-order tolerance): 15 seconds.
pub const DEFAULT_WATERMARK_DELAY_SECS: u64 = 15;

/// Default maximum number of finalized windows to retain in memory.
pub const DEFAULT_MAX_RETAINED_WINDOWS: usize = 1000;

/// Policy for handling events that arrive after their target window has been finalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LateEventPolicy {
    /// Reject from finalized window, but increment metrics and audit logs.
    DropAndRecord,
    /// Capture in side-output buffer for dedicated late-processing pipeline.
    SideOutput,
    /// Retroactively update the window and bump revision counter.
    RetroactiveUpdate,
}

/// Lifecycle state of a time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowState {
    /// Window is currently open and accepting in-order stream events.
    Active,
    /// Window has passed the watermark and is sealed/finalized.
    Finalized,
    /// Window was finalized but retroactively amended with late data.
    Amended,
}

/// Configuration for the watermarking and windowing engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatermarkConfig {
    /// Window duration in seconds.
    pub window_size_secs: u64,
    /// Out-of-order tolerance delay in seconds.
    pub watermark_delay_secs: u64,
    /// Policy for late-arriving events.
    pub late_event_policy: LateEventPolicy,
    /// Maximum retained finalized windows.
    pub max_retained_windows: usize,
    /// SLA latency threshold in milliseconds.
    pub sla_threshold_ms: f64,
    /// DDSketch relative error parameter $\alpha$.
    pub alpha: f64,
    /// Maximum allowed future clock skew in seconds.
    pub max_clock_skew_secs: u64,
}

impl Default for WatermarkConfig {
    fn default() -> Self {
        Self {
            window_size_secs: DEFAULT_WINDOW_SIZE_SECS,
            watermark_delay_secs: DEFAULT_WATERMARK_DELAY_SECS,
            late_event_policy: LateEventPolicy::DropAndRecord,
            max_retained_windows: DEFAULT_MAX_RETAINED_WINDOWS,
            sla_threshold_ms: DEFAULT_SLA_THRESHOLD_MS,
            alpha: DEFAULT_ALPHA,
            max_clock_skew_secs: DEFAULT_MAX_CLOCK_SKEW_SECS,
        }
    }
}

/// Audit record for late or anomalous events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LateEventRecord {
    pub payment_id: String,
    pub event_time: u64,
    pub watermark_at_arrival: u64,
    pub target_window_id: u64,
    pub policy_applied: LateEventPolicy,
}

/// Aggregate metrics and sketch for a single time window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowMetrics {
    pub window_id: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub state: WindowState,
    pub revision: u64,
    pub sketch: DDSketch,
    pub reliability: ReliabilityCounters,
    pub last_updated_at: u64,
}

impl WindowMetrics {
    pub fn new(window_id: u64, start_time: u64, end_time: u64, alpha: f64) -> Self {
        Self {
            window_id,
            start_time,
            end_time,
            state: WindowState::Active,
            revision: 1,
            sketch: DDSketch::new(alpha).unwrap_or_default(),
            reliability: ReliabilityCounters::new(),
            last_updated_at: start_time,
        }
    }

    /// Computes latency percentiles for this window.
    pub fn percentiles(&self) -> Option<PercentileSummary> {
        self.sketch.summary()
    }

    /// Computes reliability and SLA summary for this window.
    pub fn reliability_summary(&self) -> ReliabilitySummary {
        self.reliability.summary()
    }

    /// Computes a deterministic 32-byte hash of the window aggregate summary (for reconciliation).
    pub fn snapshot_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.window_id.to_be_bytes());
        hasher.update(self.start_time.to_be_bytes());
        hasher.update(self.end_time.to_be_bytes());
        hasher.update(self.revision.to_be_bytes());
        hasher.update(self.sketch.count().to_be_bytes());
        hasher.update(self.sketch.sum().to_be_bytes());
        hasher.update(self.reliability.total_payments.to_be_bytes());
        hasher.update(self.reliability.successful_payments.to_be_bytes());
        hasher.update(self.reliability.failed_payments.to_be_bytes());
        hasher.update(self.reliability.sla_breach_count.to_be_bytes());
        hasher.finalize().into()
    }

    /// Computes a deterministic 32-byte hash of all underlying raw sketch buckets (source data hash).
    pub fn source_data_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for (bucket_key, count) in self.sketch.bucket_entries() {
            hasher.update(bucket_key.to_be_bytes());
            hasher.update(count.to_be_bytes());
        }
        hasher.finalize().into()
    }

    /// Ingests a payment event into this window.
    pub fn ingest(&mut self, event: &PaymentEvent, sla_threshold_ms: f64) {
        let latency = event.effective_latency_ms(sla_threshold_ms);
        let _ = self.sketch.add(latency);
        let sla_breached = event.is_sla_breached(sla_threshold_ms);
        self.reliability.record(event.status, sla_breached);
        self.last_updated_at = event.ingested_at;
    }

    /// Merges another window's metrics into this window ($W = W_1 \oplus W_2$).
    pub fn merge(&mut self, other: &Self) {
        let _ = self.sketch.merge(&other.sketch);
        self.reliability.merge(&other.reliability);
        if other.last_updated_at > self.last_updated_at {
            self.last_updated_at = other.last_updated_at;
        }
    }
}

/// Result of ingesting a single payment event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IngestOutcome {
    /// Successfully incorporated into an active window.
    Incorporated { window_id: u64, is_in_order: bool },
    /// Handled according to late event policy.
    LateEventHandled {
        window_id: u64,
        policy: LateEventPolicy,
    },
    /// Event flagged with clock skew.
    ClockSkewFlagged { window_id: u64 },
}

/// Streaming watermarking engine.
#[derive(Debug, Clone)]
pub struct WatermarkTracker {
    config: WatermarkConfig,
    max_event_time: u64,
    current_watermark: u64,
    windows: BTreeMap<u64, WindowMetrics>,
    late_records: Vec<LateEventRecord>,
    late_events_total: u64,
    clock_skew_events_total: u64,
    side_output_events: Vec<PaymentEvent>,
}

impl WatermarkTracker {
    pub fn new(config: WatermarkConfig) -> Self {
        Self {
            config,
            max_event_time: 0,
            current_watermark: 0,
            windows: BTreeMap::new(),
            late_records: Vec::new(),
            late_events_total: 0,
            clock_skew_events_total: 0,
            side_output_events: Vec::new(),
        }
    }

    /// Returns the current watermark timestamp in seconds ($W(t)$).
    pub fn watermark(&self) -> u64 {
        self.current_watermark
    }

    /// Returns the maximum observed event timestamp.
    pub fn max_event_time(&self) -> u64 {
        self.max_event_time
    }

    /// Returns total number of late events observed.
    pub fn late_events_total(&self) -> u64 {
        self.late_events_total
    }

    /// Returns total number of clock skew events detected.
    pub fn clock_skew_events_total(&self) -> u64 {
        self.clock_skew_events_total
    }

    /// Computes the window ID for a given event timestamp.
    pub fn window_id_for_time(&self, timestamp: u64) -> u64 {
        timestamp / self.config.window_size_secs
    }

    /// Advances the watermark based on a newly observed event timestamp.
    pub fn advance_watermark(&mut self, event_time: u64) {
        if event_time > self.max_event_time {
            self.max_event_time = event_time;
            self.current_watermark = event_time.saturating_sub(self.config.watermark_delay_secs);
            self.update_window_finality();
        }
    }

    /// Updates window states based on the current watermark.
    fn update_window_finality(&mut self) {
        for window in self.windows.values_mut() {
            if window.state == WindowState::Active && window.end_time <= self.current_watermark {
                window.state = WindowState::Finalized;
            }
        }
        self.prune_windows();
    }

    /// Prunes old finalized windows past the retention limit.
    fn prune_windows(&mut self) {
        if self.windows.len() > self.config.max_retained_windows {
            let overflow = self.windows.len() - self.config.max_retained_windows;
            let keys_to_remove: Vec<u64> = self
                .windows
                .iter()
                .filter(|(_, w)| w.state == WindowState::Finalized || w.state == WindowState::Amended)
                .take(overflow)
                .map(|(&k, _)| k)
                .collect();

            for k in keys_to_remove {
                self.windows.remove(&k);
            }
        }
    }

    /// Ingests a payment event into the watermarked window stream.
    pub fn ingest(&mut self, event: PaymentEvent) -> IngestOutcome {
        let event_time = event.event_time();
        let window_id = self.window_id_for_time(event_time);
        let window_start = window_id * self.config.window_size_secs;
        let window_end = window_start + self.config.window_size_secs;

        // Check clock skew
        let mut is_skewed = false;
        if event.is_clock_skewed(self.config.max_clock_skew_secs) {
            self.clock_skew_events_total += 1;
            is_skewed = true;
        }

        // Check if event is arriving late (i.e. target window is already finalized)
        let is_late = window_end <= self.current_watermark;

        if is_late {
            self.late_events_total += 1;
            self.late_records.push(LateEventRecord {
                payment_id: event.payment_id.clone(),
                event_time,
                watermark_at_arrival: self.current_watermark,
                target_window_id: window_id,
                policy_applied: self.config.late_event_policy,
            });

            match self.config.late_event_policy {
                LateEventPolicy::DropAndRecord => {
                    return IngestOutcome::LateEventHandled {
                        window_id,
                        policy: LateEventPolicy::DropAndRecord,
                    };
                }
                LateEventPolicy::SideOutput => {
                    self.side_output_events.push(event);
                    return IngestOutcome::LateEventHandled {
                        window_id,
                        policy: LateEventPolicy::SideOutput,
                    };
                }
                LateEventPolicy::RetroactiveUpdate => {
                    let window = self.windows.entry(window_id).or_insert_with(|| {
                        let mut w = WindowMetrics::new(
                            window_id,
                            window_start,
                            window_end,
                            self.config.alpha,
                        );
                        w.state = WindowState::Finalized;
                        w
                    });

                    window.ingest(&event, self.config.sla_threshold_ms);
                    window.state = WindowState::Amended;
                    window.revision += 1;

                    return IngestOutcome::LateEventHandled {
                        window_id,
                        policy: LateEventPolicy::RetroactiveUpdate,
                    };
                }
            }
        }

        // Event is within acceptable watermark window (Active)
        let is_in_order = event_time >= self.max_event_time;
        self.advance_watermark(event_time);

        let window = self.windows.entry(window_id).or_insert_with(|| {
            WindowMetrics::new(window_id, window_start, window_end, self.config.alpha)
        });

        window.ingest(&event, self.config.sla_threshold_ms);

        if is_skewed {
            IngestOutcome::ClockSkewFlagged { window_id }
        } else {
            IngestOutcome::Incorporated {
                window_id,
                is_in_order,
            }
        }
    }

    /// Returns a reference to a specific window.
    pub fn get_window(&self, window_id: u64) -> Option<&WindowMetrics> {
        self.windows.get(&window_id)
    }

    /// Returns all currently active (open) windows.
    pub fn active_windows(&self) -> Vec<&WindowMetrics> {
        self.windows
            .values()
            .filter(|w| w.state == WindowState::Active)
            .collect()
    }

    /// Returns all finalized (closed) windows.
    pub fn finalized_windows(&self) -> Vec<&WindowMetrics> {
        self.windows
            .values()
            .filter(|w| w.state == WindowState::Finalized || w.state == WindowState::Amended)
            .collect()
    }

    /// Returns list of finalized window IDs (reconcilable periods for the reconciliation subsystem).
    pub fn reconcilable_periods(&self) -> Vec<u64> {
        self.finalized_windows()
            .into_iter()
            .map(|w| w.window_id)
            .collect()
    }

    /// Returns all side-output late events.
    pub fn side_output(&self) -> &[PaymentEvent] {
        &self.side_output_events
    }

    /// Merges another tracker (e.g. from a parallel ingestion worker shard).
    pub fn merge(&mut self, other: &Self) {
        if other.max_event_time > self.max_event_time {
            self.max_event_time = other.max_event_time;
            self.current_watermark = other.current_watermark;
        }

        self.late_events_total += other.late_events_total;
        self.clock_skew_events_total += other.clock_skew_events_total;
        self.late_records.extend(other.late_records.clone());
        self.side_output_events.extend(other.side_output_events.clone());

        for (&window_id, other_window) in &other.windows {
            if let Some(existing) = self.windows.get_mut(&window_id) {
                existing.merge(other_window);
            } else {
                self.windows.insert(window_id, other_window.clone());
            }
        }

        self.update_window_finality();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::reliability::PaymentStatus;

    #[test]
    fn test_watermark_in_order_stream() {
        let config = WatermarkConfig {
            window_size_secs: 60,
            watermark_delay_secs: 15,
            late_event_policy: LateEventPolicy::DropAndRecord,
            ..Default::default()
        };

        let mut tracker = WatermarkTracker::new(config);

        // Ingest events at t=10, 30, 50, 70, 90
        for &t in &[10, 30, 50, 70, 90] {
            let event = PaymentEvent {
                payment_id: format!("tx_{}", t),
                ledger_sequence: t,
                ledger_closed_at: t,
                client_submitted_at: Some(t - 1),
                ingested_at: t + 1,
                status: PaymentStatus::Success,
                corridor: None,
                latency_ms: Some(100.0),
            };
            tracker.ingest(event);
        }

        // At t=90, watermark is 90 - 15 = 75.
        assert_eq!(tracker.watermark(), 75);
        assert_eq!(tracker.max_event_time(), 90);

        // Window 0 (0..60) has end_time 60 <= 75 -> Finalized
        // Window 1 (60..120) has end_time 120 > 75 -> Active
        let w0 = tracker.get_window(0).unwrap();
        assert_eq!(w0.state, WindowState::Finalized);
        assert_eq!(w0.sketch.count(), 3); // events 10, 30, 50

        let w1 = tracker.get_window(1).unwrap();
        assert_eq!(w1.state, WindowState::Active);
        assert_eq!(w1.sketch.count(), 2); // events 70, 90

        let reconcilable = tracker.reconcilable_periods();
        assert_eq!(reconcilable, vec![0]);
    }

    #[test]
    fn test_out_of_order_within_watermark_tolerance() {
        let config = WatermarkConfig {
            window_size_secs: 60,
            watermark_delay_secs: 20, // 20s delay tolerance
            late_event_policy: LateEventPolicy::DropAndRecord,
            ..Default::default()
        };

        let mut tracker = WatermarkTracker::new(config);

        // Max time advances to 50
        tracker.ingest(PaymentEvent {
            payment_id: "tx_50".into(),
            ledger_sequence: 50,
            ledger_closed_at: 50,
            client_submitted_at: None,
            ingested_at: 51,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(200.0),
        });

        // Watermark is 50 - 20 = 30.
        assert_eq!(tracker.watermark(), 30);

        // An out-of-order event arrives with t=35 (belongs to Window 0, 0..60)
        // Since window_end (60) > watermark (30), Window 0 is still Active!
        let outcome = tracker.ingest(PaymentEvent {
            payment_id: "tx_35".into(),
            ledger_sequence: 35,
            ledger_closed_at: 35,
            client_submitted_at: None,
            ingested_at: 52,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(150.0),
        });

        match outcome {
            IngestOutcome::Incorporated {
                window_id,
                is_in_order,
            } => {
                assert_eq!(window_id, 0);
                assert!(!is_in_order); // Correctly recognized as out-of-order
            }
            _ => panic!("Expected Incorporated"),
        }

        let w0 = tracker.get_window(0).unwrap();
        assert_eq!(w0.sketch.count(), 2);
    }

    #[test]
    fn test_late_event_drop_and_record() {
        let config = WatermarkConfig {
            window_size_secs: 60,
            watermark_delay_secs: 10,
            late_event_policy: LateEventPolicy::DropAndRecord,
            ..Default::default()
        };

        let mut tracker = WatermarkTracker::new(config);

        // Advance watermark past Window 0 (t=100 -> watermark 90 > window_end 60)
        tracker.ingest(PaymentEvent {
            payment_id: "tx_100".into(),
            ledger_sequence: 100,
            ledger_closed_at: 100,
            client_submitted_at: None,
            ingested_at: 100,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(100.0),
        });

        assert_eq!(tracker.watermark(), 90);

        // Late event arrives with t=20 (Window 0 is already finalized)
        let outcome = tracker.ingest(PaymentEvent {
            payment_id: "tx_late_20".into(),
            ledger_sequence: 20,
            ledger_closed_at: 20,
            client_submitted_at: None,
            ingested_at: 101,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(500.0),
        });

        assert_eq!(
            outcome,
            IngestOutcome::LateEventHandled {
                window_id: 0,
                policy: LateEventPolicy::DropAndRecord,
            }
        );

        assert_eq!(tracker.late_events_total(), 1);
    }

    #[test]
    fn test_late_event_retroactive_update() {
        let config = WatermarkConfig {
            window_size_secs: 60,
            watermark_delay_secs: 10,
            late_event_policy: LateEventPolicy::RetroactiveUpdate,
            ..Default::default()
        };

        let mut tracker = WatermarkTracker::new(config);

        // Ingest initial event in Window 0
        tracker.ingest(PaymentEvent {
            payment_id: "tx_10".into(),
            ledger_sequence: 10,
            ledger_closed_at: 10,
            client_submitted_at: None,
            ingested_at: 10,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(100.0),
        });

        // Advance watermark to 90 (finalizing Window 0)
        tracker.advance_watermark(100);
        let w0 = tracker.get_window(0).unwrap();
        assert_eq!(w0.state, WindowState::Finalized);
        assert_eq!(w0.revision, 1);

        // Late event arrives for Window 0
        let outcome = tracker.ingest(PaymentEvent {
            payment_id: "tx_late_30".into(),
            ledger_sequence: 30,
            ledger_closed_at: 30,
            client_submitted_at: None,
            ingested_at: 105,
            status: PaymentStatus::Success,
            corridor: None,
            latency_ms: Some(200.0),
        });

        assert_eq!(
            outcome,
            IngestOutcome::LateEventHandled {
                window_id: 0,
                policy: LateEventPolicy::RetroactiveUpdate,
            }
        );

        let w0_amended = tracker.get_window(0).unwrap();
        assert_eq!(w0_amended.state, WindowState::Amended);
        assert_eq!(w0_amended.revision, 2);
        assert_eq!(w0_amended.sketch.count(), 2);
    }
}
