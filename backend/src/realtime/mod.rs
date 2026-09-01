pub mod connection;
pub mod fanout;
pub mod policy;

pub use connection::{Connection, ConnectionReceiver};
pub use fanout::{
    fanout_message, fanout_message_detailed, fanout_message_with_concurrency, FanoutConfig,
    FanoutSummary,
};
pub use policy::{
    ConnectionHealthState, ConnectionPolicyTracker, PolicyDecision, QueueDepthTrend,
    SlowConsumerPolicyConfig,
};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::protocol::Message;

pub type ConnectionId = String;
pub type ConnectionSender = tokio::sync::mpsc::UnboundedSender<Message>;

/// Thread-safe registry of active WebSocket client connections.
#[derive(Clone)]
pub struct ConnectionRegistry {
    pub connections: Arc<RwLock<HashMap<ConnectionId, Arc<Connection>>>>,
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionRegistry {
    /// Creates an empty connection registry.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers a connection by raw sender channel, wrapping it in a managed `Connection`.
    pub async fn add(&self, id: ConnectionId, sender: ConnectionSender) {
        let conn = Arc::new(Connection::new_with_sender(
            id.clone(),
            sender,
            SlowConsumerPolicyConfig::default(),
        ));
        let mut guard = self.connections.write().await;
        guard.insert(id, conn);
    }

    /// Registers a pre-configured `Arc<Connection>`.
    pub async fn add_connection(&self, connection: Arc<Connection>) {
        let mut guard = self.connections.write().await;
        guard.insert(connection.id.clone(), connection);
    }

    /// Removes a connection by ID.
    pub async fn remove(&self, id: &str) -> Option<Arc<Connection>> {
        let mut guard = self.connections.write().await;
        guard.remove(id)
    }

    /// Removes a batch of connections in a single write-lock acquisition.
    pub async fn remove_batch(&self, ids: &[ConnectionId]) {
        let mut guard = self.connections.write().await;
        for id in ids {
            guard.remove(id);
        }
    }

    /// Retrieves an individual connection if registered.
    pub async fn get(&self, id: &str) -> Option<Arc<Connection>> {
        let guard = self.connections.read().await;
        guard.get(id).cloned()
    }

    /// Backward-compatible method returning snapshot of `(id, sender)` pairs.
    pub async fn get_all(&self) -> Vec<(ConnectionId, ConnectionSender)> {
        let guard = self.connections.read().await;
        guard
            .iter()
            .map(|(k, v)| (k.clone(), v.sender.clone()))
            .collect()
    }

    /// Returns a snapshot of all active `Arc<Connection>` handles.
    pub async fn get_all_connections(&self) -> Vec<Arc<Connection>> {
        let guard = self.connections.read().await;
        guard.values().cloned().collect()
    }

    /// Returns the number of currently registered connections.
    pub async fn len(&self) -> usize {
        let guard = self.connections.read().await;
        guard.len()
    }

    /// Returns whether the registry is empty.
    pub async fn is_empty(&self) -> bool {
        let guard = self.connections.read().await;
        guard.is_empty()
    }
}
