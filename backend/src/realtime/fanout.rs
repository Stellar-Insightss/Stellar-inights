use std::sync::Arc;
use std::time::Duration;
use futures_util::stream::{self, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::realtime::policy::handle_overflow;
use crate::realtime::{Connection, ConnectionId, ConnectionRegistry};

/// Configuration parameters for realtime message fanout.
#[derive(Debug, Clone)]
pub struct FanoutConfig {
    /// Maximum concurrent sends allowed in flight.
    pub max_concurrency: usize,
    /// Per-connection timeout for delivery before isolating.
    pub per_connection_timeout: Option<Duration>,
}

impl Default for FanoutConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 64,
            per_connection_timeout: Some(Duration::from_millis(50)),
        }
    }
}

/// Telemetry summary returned after a fanout operation completes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FanoutSummary {
    pub total_connections: usize,
    pub delivered: usize,
    pub quarantined_dropped: usize,
    pub failed_evicted: usize,
}

enum DeliveryResult {
    Delivered,
    QuarantinedDropped,
    Evict(ConnectionId),
}

/// Fans out a message to all connected clients in the registry with bounded concurrency and per-connection isolation.
pub async fn fanout_message(
    registry: &ConnectionRegistry,
    message: Message,
) -> Result<(), String> {
    fanout_message_with_concurrency(registry, message, 64).await
}

/// Fans out a message with an explicit concurrency bound.
pub async fn fanout_message_with_concurrency(
    registry: &ConnectionRegistry,
    message: Message,
    max_concurrency: usize,
) -> Result<(), String> {
    let config = FanoutConfig {
        max_concurrency,
        per_connection_timeout: None,
    };
    fanout_message_detailed(registry, &message, &config)
        .await
        .map(|_| ())
}

/// Detailed message fanout returning delivery metrics and offloading slow-consumer eviction to a background task.
pub async fn fanout_message_detailed(
    registry: &ConnectionRegistry,
    message: &Message,
    config: &FanoutConfig,
) -> Result<FanoutSummary, String> {
    let connections = registry.get_all_connections().await;
    let total_connections = connections.len();

    if total_connections == 0 {
        return Ok(FanoutSummary::default());
    }

    let concurrency = config.max_concurrency.max(1);

    // Deliver concurrently across connections bounded by `concurrency`
    let delivery_results: Vec<DeliveryResult> = stream::iter(connections)
        .map(|conn: Arc<Connection>| {
            let msg = message.clone();
            let timeout_opt = config.per_connection_timeout;
            async move {
                let send_future = conn.send(msg);

                let result = match timeout_opt {
                    Some(timeout) => match tokio::time::timeout(timeout, send_future).await {
                        Ok(send_res) => send_res,
                        Err(_) => Err("Send timed out".to_string()),
                    },
                    None => send_future.await,
                };

                match result {
                    Ok(_) => DeliveryResult::Delivered,
                    Err(_) => {
                        let should_evict = conn.should_evict().await;
                        if should_evict {
                            DeliveryResult::Evict(conn.id.clone())
                        } else if conn.is_quarantined().await {
                            DeliveryResult::QuarantinedDropped
                        } else {
                            DeliveryResult::Evict(conn.id.clone())
                        }
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    let mut summary = FanoutSummary {
        total_connections,
        delivered: 0,
        quarantined_dropped: 0,
        failed_evicted: 0,
    };

    let mut to_evict = Vec::new();

    for result in delivery_results {
        match result {
            DeliveryResult::Delivered => summary.delivered += 1,
            DeliveryResult::QuarantinedDropped => summary.quarantined_dropped += 1,
            DeliveryResult::Evict(id) => {
                summary.failed_evicted += 1;
                to_evict.push(id);
            }
        }
    }

    // Offload eviction and overflow handling off the hot path
    if !to_evict.is_empty() {
        let registry_clone = registry.clone();
        tokio::spawn(async move {
            for id in &to_evict {
                let _ = handle_overflow(id).await;
            }
            registry_clone.remove_batch(&to_evict).await;
        });
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_fanout() {
        let registry = ConnectionRegistry::new();
        let summary = fanout_message_detailed(&registry, &Message::text("test"), &FanoutConfig::default())
            .await
            .unwrap();
        assert_eq!(summary, FanoutSummary::default());
    }

    #[tokio::test]
    async fn test_healthy_fanout() {
        let registry = ConnectionRegistry::new();
        let mut rxs = Vec::new();

        for i in 0..10 {
            let (conn, rx) = Connection::create_channel(format!("conn-{}", i));
            registry.add_connection(Arc::new(conn)).await;
            rxs.push(rx);
        }

        let summary = fanout_message_detailed(&registry, &Message::text("hello"), &FanoutConfig::default())
            .await
            .unwrap();

        assert_eq!(summary.total_connections, 10);
        assert_eq!(summary.delivered, 10);
        assert_eq!(summary.failed_evicted, 0);

        for mut rx in rxs {
            let msg = rx.recv().await.unwrap();
            assert_eq!(msg, Message::text("hello"));
        }
    }
}