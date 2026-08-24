pub mod connection;
pub mod fanout;
pub mod policy;

use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use tokio_tungstenite::tungstenite::protocol::Message;

pub type ConnectionId = String;
pub type ConnectionSender = tokio::sync::mpsc::UnboundedSender<Message>;

pub struct ConnectionRegistry {
    pub connections: Arc<Mutex<HashMap<ConnectionId, ConnectionSender>>>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add(&self, id: ConnectionId, sender: ConnectionSender) {
        let mut guard = self.connections.lock().await;
        guard.insert(id, sender);
    }

    pub async fn remove(&self, id: &str) {
        let mut guard = self.connections.lock().await;
        guard.remove(id);
    }

    pub async fn get_all(&self) -> Vec<(ConnectionId, ConnectionSender)> {
        let guard = self.connections.lock().await;
        guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}