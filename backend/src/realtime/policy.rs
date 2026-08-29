use std::collections::VecDeque;
use std::time::{Duration, Instant};
use log::{info, warn};
use prometheus::{IntCounter, IntGauge, register_int_counter, register_int_gauge};
use serde::{Deserialize, Serialize};

lazy_static::lazy_static! {
    pub static ref SLOW_CONSUMER_COUNT: IntCounter = register_int_counter!(
        "slow_consumer_disconnects_total",
        "Total number of slow consumer disconnections"
    ).unwrap();

    pub static ref QUARANTINED_CONSUMER_COUNT: IntCounter = register_int_counter!(
        "quarantined_consumers_total",
        "Total number of slow consumer quarantines"
    ).unwrap();

    pub static ref DROPPED_MESSAGES_COUNT: IntCounter = register_int_counter!(
        "dropped_messages_total",
        "Total number of messages dropped due to slow consumers"
    ).unwrap();

    pub static ref QUEUE_DEPTH_GAUGE: IntGauge = register_int_gauge!(
        "connection_queue_depth",
        "Current queue depth per connection"
    ).unwrap();
}

/// Health state of a connection in the realtime subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionHealthState {
    /// Normal operation: healthy drain rate and low queue depth.
    Healthy,
    /// Queue depth is elevated or growing; monitored for quarantine.
    Degraded,
    /// Connection is quarantined: messages are dropped to isolate healthy connections.
    Quarantined,
    /// Connection exceeded maximum queue depth or dropped message limits and is evicted.
    Evicted,
}

/// Directional trend of queue depth over a sliding observation window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueDepthTrend {
    /// Queue is empty or stable at a low depth.
    Stable,
    /// Consumer is draining faster than ingestion.
    Draining,
    /// Queue depth is increasing over successive samples.
    Growing,
    /// Queue is full or backed up with no drain progress.
    Stalled,
}

/// Decision made by the policy tracker for an incoming message delivery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Allow message delivery normally.
    Allow,
    /// Allow message delivery, but connection is flagged as degraded.
    AllowDegraded,
    /// Drop message to prevent head-of-line blocking while connection is quarantined.
    QuarantineDrop,
    /// Evict connection immediately.
    Evict,
}

/// Single queue depth sample timestamped for trend analysis.
#[derive(Debug, Clone, Copy)]
pub struct QueueSample {
    pub timestamp: Instant,
    pub depth: usize,
}

/// Configuration parameters for the proactive slow-consumer policy.
#[derive(Debug, Clone)]
pub struct SlowConsumerPolicyConfig {
    /// Maximum allowable queue depth before immediate eviction.
    pub max_queue_depth: usize,
    /// Queue depth threshold where connection becomes Degraded.
    pub warning_queue_depth: usize,
    /// Number of consecutive high/growing samples required before quarantining.
    pub consecutive_high_samples_threshold: usize,
    /// Size of the trailing queue depth observation window.
    pub sample_window_size: usize,
    /// Leaky bucket leak rate in tokens (burst units) per second.
    pub leak_rate_per_sec: f64,
    /// Leaky bucket burst capacity to prevent false-positive evictions during transient traffic bursts.
    pub bucket_capacity: f64,
    /// Cooldown duration in quarantine before recovery is permitted.
    pub quarantine_cooldown: Duration,
    /// Maximum consecutive dropped messages in quarantine before hard eviction.
    pub max_quarantined_drops: usize,
}

impl Default for SlowConsumerPolicyConfig {
    fn default() -> Self {
        Self {
            max_queue_depth: 100,
            warning_queue_depth: 25,
            consecutive_high_samples_threshold: 3,
            sample_window_size: 10,
            leak_rate_per_sec: 10.0,
            bucket_capacity: 30.0,
            quarantine_cooldown: Duration::from_millis(500),
            max_quarantined_drops: 20,
        }
    }
}

/// Per-connection tracker maintaining queue depth trend, leaky bucket state, and health lifecycle.
#[derive(Debug)]
pub struct ConnectionPolicyTracker {
    config: SlowConsumerPolicyConfig,
    history: VecDeque<QueueSample>,
    state: ConnectionHealthState,
    consecutive_high_samples: usize,
    quarantined_at: Option<Instant>,
    quarantined_drops: usize,
    leaky_bucket_level: f64,
    last_bucket_update: Instant,
    total_sent: u64,
    total_dropped: u64,
}

impl ConnectionPolicyTracker {
    pub fn new(config: SlowConsumerPolicyConfig) -> Self {
        let now = Instant::now();
        Self {
            config,
            history: VecDeque::with_capacity(16),
            state: ConnectionHealthState::Healthy,
            consecutive_high_samples: 0,
            quarantined_at: None,
            quarantined_drops: 0,
            leaky_bucket_level: 0.0,
            last_bucket_update: now,
            total_sent: 0,
            total_dropped: 0,
        }
    }

    /// Evaluates current queue depth and returns a policy decision for the connection.
    pub fn record_queue_depth(&mut self, current_depth: usize) -> PolicyDecision {
        let now = Instant::now();

        // 1. Update leaky bucket
        let elapsed_secs = now.duration_since(self.last_bucket_update).as_secs_f64();
        self.leaky_bucket_level = (self.leaky_bucket_level - elapsed_secs * self.config.leak_rate_per_sec).max(0.0);
        self.last_bucket_update = now;

        // 2. Record historical sample
        if self.history.len() >= self.config.sample_window_size {
            self.history.pop_front();
        }
        self.history.push_back(QueueSample {
            timestamp: now,
            depth: current_depth,
        });

        // 3. Immediate hard eviction check
        if current_depth >= self.config.max_queue_depth {
            self.state = ConnectionHealthState::Evicted;
            return PolicyDecision::Evict;
        }

        // 4. Handle quarantined state
        if self.state == ConnectionHealthState::Quarantined {
            if let Some(q_time) = self.quarantined_at {
                // If queue has drained back to healthy levels and cooldown elapsed, recover
                if current_depth <= self.config.warning_queue_depth / 2
                    && now.duration_since(q_time) >= self.config.quarantine_cooldown
                {
                    info!("Connection recovered from quarantine (queue depth: {})", current_depth);
                    self.state = ConnectionHealthState::Healthy;
                    self.quarantined_at = None;
                    self.quarantined_drops = 0;
                    self.consecutive_high_samples = 0;
                    self.leaky_bucket_level = 0.0;
                    self.total_sent += 1;
                    return PolicyDecision::Allow;
                }
            }

            // Still quarantined
            if self.quarantined_drops >= self.config.max_quarantined_drops {
                self.state = ConnectionHealthState::Evicted;
                return PolicyDecision::Evict;
            }

            self.quarantined_drops += 1;
            self.total_dropped += 1;
            DROPPED_MESSAGES_COUNT.inc();
            return PolicyDecision::QuarantineDrop;
        }

        // 5. Evaluate depth & growth trend for healthy / degraded
        let trend = self.calculate_trend();

        if current_depth >= self.config.warning_queue_depth {
            self.consecutive_high_samples += 1;
            self.leaky_bucket_level = (self.leaky_bucket_level + 1.0).min(self.config.bucket_capacity + 10.0);

            // Proactive quarantine trigger: sustained high samples or bucket overflow with growing trend
            let should_quarantine = self.consecutive_high_samples >= self.config.consecutive_high_samples_threshold
                || (self.leaky_bucket_level >= self.config.bucket_capacity && trend == QueueDepthTrend::Growing);

            if should_quarantine {
                warn!(
                    "Connection transitioned to Quarantined (depth: {}, trend: {:?}, consecutive: {})",
                    current_depth, trend, self.consecutive_high_samples
                );
                self.state = ConnectionHealthState::Quarantined;
                self.quarantined_at = Some(now);
                self.quarantined_drops = 1;
                self.total_dropped += 1;
                QUARANTINED_CONSUMER_COUNT.inc();
                DROPPED_MESSAGES_COUNT.inc();
                return PolicyDecision::QuarantineDrop;
            }

            self.state = ConnectionHealthState::Degraded;
            self.total_sent += 1;
            return PolicyDecision::AllowDegraded;
        }

        // Low depth
        self.consecutive_high_samples = 0;
        self.state = ConnectionHealthState::Healthy;
        self.total_sent += 1;
        PolicyDecision::Allow
    }

    /// Records an explicit message drop.
    pub fn record_drop(&mut self) -> PolicyDecision {
        self.total_dropped += 1;
        DROPPED_MESSAGES_COUNT.inc();
        if self.state == ConnectionHealthState::Quarantined {
            self.quarantined_drops += 1;
            if self.quarantined_drops >= self.config.max_quarantined_drops {
                self.state = ConnectionHealthState::Evicted;
                return PolicyDecision::Evict;
            }
            return PolicyDecision::QuarantineDrop;
        }
        self.state = ConnectionHealthState::Evicted;
        PolicyDecision::Evict
    }

    /// Records drain progress when the consumer consumes messages.
    pub fn record_drain(&mut self, amount: usize) {
        if let Some(last) = self.history.back_mut() {
            last.depth = last.depth.saturating_sub(amount);
        }
        self.leaky_bucket_level = (self.leaky_bucket_level - amount as f64).max(0.0);
    }

    /// Computes queue depth trend across recorded samples.
    pub fn calculate_trend(&self) -> QueueDepthTrend {
        if self.history.len() < 2 {
            return QueueDepthTrend::Stable;
        }

        let last = self.history.back().unwrap().depth;
        if last == 0 {
            return QueueDepthTrend::Stable;
        }
        if last >= self.config.max_queue_depth {
            return QueueDepthTrend::Stalled;
        }

        let n = self.history.len();
        let prev = self.history[n - 2].depth;

        if last < prev {
            QueueDepthTrend::Draining
        } else if last > prev {
            QueueDepthTrend::Growing
        } else {
            let first = self.history.front().unwrap().depth;
            if last < first {
                QueueDepthTrend::Draining
            } else if last > first {
                QueueDepthTrend::Growing
            } else {
                QueueDepthTrend::Stable
            }
        }
    }

    pub fn health_state(&self) -> ConnectionHealthState {
        self.state
    }

    pub fn is_quarantined(&self) -> bool {
        self.state == ConnectionHealthState::Quarantined
    }

    pub fn should_evict(&self) -> bool {
        self.state == ConnectionHealthState::Evicted
    }

    pub fn quarantined_drops(&self) -> usize {
        self.quarantined_drops
    }

    pub fn total_sent(&self) -> u64 {
        self.total_sent
    }

    pub fn total_dropped(&self) -> u64 {
        self.total_dropped
    }

    pub fn config(&self) -> &SlowConsumerPolicyConfig {
        &self.config
    }
}

/// Handles slow consumer overflow off the hot path.
pub async fn handle_overflow(connection_id: &str) -> Result<(), String> {
    warn!("Slow consumer detected: {} - disconnecting", connection_id);
    SLOW_CONSUMER_COUNT.inc();
    info!("Disconnected slow consumer: {}", connection_id);
    QUEUE_DEPTH_GAUGE.set(0);
    Ok(())
}

/// Handles quarantine transition for logging and metrics.
pub async fn handle_quarantine(connection_id: &str) -> Result<(), String> {
    warn!("Slow consumer quarantined: {}", connection_id);
    QUARANTINED_CONSUMER_COUNT.inc();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_tracker_healthy_flow() {
        let mut tracker = ConnectionPolicyTracker::new(SlowConsumerPolicyConfig::default());
        assert_eq!(tracker.health_state(), ConnectionHealthState::Healthy);

        for depth in [1, 2, 3, 2, 1, 0] {
            let decision = tracker.record_queue_depth(depth);
            assert_eq!(decision, PolicyDecision::Allow);
            assert_eq!(tracker.health_state(), ConnectionHealthState::Healthy);
        }
    }

    #[test]
    fn test_policy_tracker_trend_detection() {
        let mut tracker = ConnectionPolicyTracker::new(SlowConsumerPolicyConfig::default());
        tracker.record_queue_depth(2);
        tracker.record_queue_depth(5);
        tracker.record_queue_depth(10);
        assert_eq!(tracker.calculate_trend(), QueueDepthTrend::Growing);

        tracker.record_queue_depth(4);
        assert_eq!(tracker.calculate_trend(), QueueDepthTrend::Draining);
    }

    #[test]
    fn test_policy_tracker_quarantine_and_recovery() {
        let config = SlowConsumerPolicyConfig {
            warning_queue_depth: 10,
            consecutive_high_samples_threshold: 2,
            quarantine_cooldown: Duration::from_millis(5),
            ..Default::default()
        };
        let mut tracker = ConnectionPolicyTracker::new(config);

        // Sample 1 at high depth: degraded
        let d1 = tracker.record_queue_depth(12);
        assert_eq!(d1, PolicyDecision::AllowDegraded);
        assert_eq!(tracker.health_state(), ConnectionHealthState::Degraded);

        // Sample 2 at high depth: quarantined
        let d2 = tracker.record_queue_depth(15);
        assert_eq!(d2, PolicyDecision::QuarantineDrop);
        assert_eq!(tracker.health_state(), ConnectionHealthState::Quarantined);
        assert!(tracker.is_quarantined());

        // Wait cooldown and drain
        std::thread::sleep(Duration::from_millis(10));
        let d3 = tracker.record_queue_depth(2);
        assert_eq!(d3, PolicyDecision::Allow);
        assert_eq!(tracker.health_state(), ConnectionHealthState::Healthy);
    }

    #[test]
    fn test_policy_tracker_max_depth_eviction() {
        let config = SlowConsumerPolicyConfig {
            max_queue_depth: 50,
            ..Default::default()
        };
        let mut tracker = ConnectionPolicyTracker::new(config);
        let decision = tracker.record_queue_depth(50);
        assert_eq!(decision, PolicyDecision::Evict);
        assert!(tracker.should_evict());
    }

    #[test]
    fn test_transient_burst_does_not_evict() {
        let config = SlowConsumerPolicyConfig {
            warning_queue_depth: 20,
            consecutive_high_samples_threshold: 3,
            bucket_capacity: 50.0,
            ..Default::default()
        };
        let mut tracker = ConnectionPolicyTracker::new(config);

        // Transient single burst
        let d1 = tracker.record_queue_depth(25);
        assert_eq!(d1, PolicyDecision::AllowDegraded);

        // Immediately drained
        tracker.record_drain(20);
        let d2 = tracker.record_queue_depth(5);
        assert_eq!(d2, PolicyDecision::Allow);
        assert_eq!(tracker.health_state(), ConnectionHealthState::Healthy);
    }
}