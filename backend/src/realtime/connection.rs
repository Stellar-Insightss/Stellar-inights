use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc::{self, UnboundedReceiver, UnboundedSender}, Mutex};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::realtime::policy::{
    ConnectionHealthState, ConnectionPolicyTracker, PolicyDecision, QueueDepthTrend,
    SlowConsumerPolicyConfig,
};

pub type ConnectionId = String;

/// Active WebSocket client connection with isolated queue depth tracking and proactive slow-consumer policy.
pub struct Connection {
    pub id: ConnectionId,
    pub sender: UnboundedSender<Message>,
    pub queue_depth: Arc<AtomicUsize>,
    pub policy_tracker: Arc<Mutex<ConnectionPolicyTracker>>,
}

impl Connection {
    /// Creates a new connection with default policy and unbound channel.
    pub fn new(id: ConnectionId) -> Self {
        Self::with_policy(id, SlowConsumerPolicyConfig::default())
    }

    /// Creates a new connection with specified policy configuration.
    pub fn with_policy(id: ConnectionId, config: SlowConsumerPolicyConfig) -> Self {
        let (sender, _receiver) = mpsc::unbounded_channel();
        Self {
            id,
            sender,
            queue_depth: Arc::new(AtomicUsize::new(0)),
            policy_tracker: Arc::new(Mutex::new(ConnectionPolicyTracker::new(config))),
        }
    }

    /// Creates a connection wrapping an existing sender.
    pub fn new_with_sender(
        id: ConnectionId,
        sender: UnboundedSender<Message>,
        config: SlowConsumerPolicyConfig,
    ) -> Self {
        Self {
            id,
            sender,
            queue_depth: Arc::new(AtomicUsize::new(0)),
            policy_tracker: Arc::new(Mutex::new(ConnectionPolicyTracker::new(config))),
        }
    }

    /// Creates a connected pair of `(Connection, ConnectionReceiver)` with automatic queue depth accounting.
    pub fn create_channel(id: ConnectionId) -> (Self, ConnectionReceiver) {
        Self::create_channel_with_policy(id, SlowConsumerPolicyConfig::default())
    }

    /// Creates a connected pair with custom policy.
    pub fn create_channel_with_policy(
        id: ConnectionId,
        config: SlowConsumerPolicyConfig,
    ) -> (Self, ConnectionReceiver) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let queue_depth = Arc::new(AtomicUsize::new(0));
        let policy_tracker = Arc::new(Mutex::new(ConnectionPolicyTracker::new(config)));

        let connection = Self {
            id: id.clone(),
            sender,
            queue_depth: queue_depth.clone(),
            policy_tracker: policy_tracker.clone(),
        };

        let connection_receiver = ConnectionReceiver {
            id,
            receiver,
            queue_depth,
            policy_tracker,
        };

        (connection, connection_receiver)
    }

    /// Non-blocking, isolated send method evaluated against the proactive slow-consumer policy.
    pub async fn send(&self, message: Message) -> Result<(), String> {
        let current_depth = self.queue_depth.load(Ordering::Relaxed);

        let decision = {
            let mut tracker = self.policy_tracker.lock().await;
            tracker.record_queue_depth(current_depth)
        };

        match decision {
            PolicyDecision::Allow | PolicyDecision::AllowDegraded => {
                self.queue_depth.fetch_add(1, Ordering::SeqCst);
                match self.sender.send(message) {
                    Ok(_) => Ok(()),
                    Err(_) => {
                        self.queue_depth.fetch_sub(1, Ordering::SeqCst);
                        let mut tracker = self.policy_tracker.lock().await;
                        tracker.record_drop();
                        Err("Failed to send message: receiver disconnected".to_string())
                    }
                }
            }
            PolicyDecision::QuarantineDrop => {
                Err("Connection quarantined: message dropped to protect realtime fanout latency".to_string())
            }
            PolicyDecision::Evict => {
                Err("Connection evicted: exceeded queue depth limit".to_string())
            }
        }
    }

    /// Current queue depth.
    pub fn queue_depth(&self) -> usize {
        self.queue_depth.load(Ordering::Relaxed)
    }

    /// Manually decrement queue depth when a message is processed externally.
    pub fn decrement_queue_depth(&self) {
        if self.queue_depth.load(Ordering::Relaxed) > 0 {
            self.queue_depth.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Returns current health state.
    pub async fn health_state(&self) -> ConnectionHealthState {
        let tracker = self.policy_tracker.lock().await;
        tracker.health_state()
    }

    /// Returns whether the connection is currently quarantined.
    pub async fn is_quarantined(&self) -> bool {
        let tracker = self.policy_tracker.lock().await;
        tracker.is_quarantined()
    }

    /// Returns whether the connection should be evicted from the registry.
    pub async fn should_evict(&self) -> bool {
        let tracker = self.policy_tracker.lock().await;
        tracker.should_evict()
    }

    /// Returns the trend in queue depth.
    pub async fn trend(&self) -> QueueDepthTrend {
        let tracker = self.policy_tracker.lock().await;
        tracker.calculate_trend()
    }
}

/// Paired receiver that drains messages and automatically decrements the queue depth tracker.
pub struct ConnectionReceiver {
    pub id: ConnectionId,
    pub receiver: UnboundedReceiver<Message>,
    pub queue_depth: Arc<AtomicUsize>,
    pub policy_tracker: Arc<Mutex<ConnectionPolicyTracker>>,
}

impl ConnectionReceiver {
    /// Asynchronously receives the next message, draining queue depth and recording telemetry.
    pub async fn recv(&mut self) -> Option<Message> {
        let msg = self.receiver.recv().await?;
        if self.queue_depth.load(Ordering::Relaxed) > 0 {
            self.queue_depth.fetch_sub(1, Ordering::SeqCst);
        }
        let current_depth = self.queue_depth.load(Ordering::Relaxed);
        let mut tracker = self.policy_tracker.lock().await;
        tracker.record_drain(1);
        tracker.record_queue_depth(current_depth);
        Some(msg)
    }

    /// Non-blocking receive attempt.
    pub fn try_recv(&mut self) -> Result<Message, mpsc::error::TryRecvError> {
        let msg = self.receiver.try_recv()?;
        if self.queue_depth.load(Ordering::Relaxed) > 0 {
            self.queue_depth.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_send_and_receive() {
        let (conn, mut rx) = Connection::create_channel("test-conn".to_string());
        assert_eq!(conn.queue_depth(), 0);

        conn.send(Message::text("hello")).await.unwrap();
        assert_eq!(conn.queue_depth(), 1);

        let received = rx.recv().await.unwrap();
        assert_eq!(received, Message::text("hello"));
        assert_eq!(conn.queue_depth(), 0);
    }

    #[tokio::test]
    async fn test_connection_eviction_on_receiver_drop() {
        let (conn, rx) = Connection::create_channel("test-conn".to_string());
        drop(rx);

        let result = conn.send(Message::text("hello")).await;
        assert!(result.is_err());
        assert!(conn.should_evict().await);
    }
}