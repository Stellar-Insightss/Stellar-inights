use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{connect_async, tungstenite::Error};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::realtime::policy::handle_overflow;

pub type ConnectionId = String;

pub struct Connection {
    pub id: ConnectionId,
    pub sender: UnboundedSender<Message>,
    pub queue_depth: Arc<Mutex<usize>>,
}

impl Connection {
    pub fn new(id: ConnectionId) -> Self {
        let (sender, _receiver) = mpsc::unbounded_channel();
        Self {
            id,
            sender,
            queue_depth: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn send(&self, message: Message) -> Result<(), String> {
        let mut depth = self.queue_depth.lock().await;
        *depth += 1;

        // If queue depth exceeds a threshold, apply overflow policy
        if *depth > 100 {
            let _ = handle_overflow(&self.id).await;
            return Err("Queue full".to_string());
        }

        match self.sender.send(message) {
            Ok(_) => {
                *depth -= 1;
                Ok(())
            }
            Err(_) => {
                *depth -= 1;
                Err("Failed to send message".to_string())
            }
        }
    }
}