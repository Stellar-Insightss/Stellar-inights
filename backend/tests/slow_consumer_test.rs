#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use stellar_insights_backend::realtime::{
        fanout_message, fanout_message_detailed, Connection, ConnectionHealthState,
        ConnectionRegistry, FanoutConfig, QueueDepthTrend, SlowConsumerPolicyConfig,
    };
    use tokio::time::sleep;
    use tokio_tungstenite::tungstenite::protocol::Message;

    /// Acceptance Criterion 1:
    /// A stalled/slow single connection cannot measurably delay delivery to any other healthy connection.
    /// Demonstrates that p99 latency to N-1 healthy connections remains low (< 15ms) even when a stalled
    /// connection has a full buffer / does not drain.
    #[tokio::test]
    async fn test_stalled_connection_does_not_delay_healthy_connections() {
        let registry = ConnectionRegistry::new();
        let healthy_count = 50;
        let mut healthy_receivers = Vec::new();

        // 1. Set up healthy consumers with active draining tasks
        for i in 0..healthy_count {
            let id = format!("healthy-{}", i);
            let (conn, rx) = Connection::create_channel(id);
            registry.add_connection(Arc::new(conn)).await;
            healthy_receivers.push(rx);
        }

        // 2. Set up 1 deliberately stalled consumer (policy with low warning depth, receiver never drains)
        let stalled_id = "stalled-consumer".to_string();
        let stalled_config = SlowConsumerPolicyConfig {
            warning_queue_depth: 2,
            consecutive_high_samples_threshold: 2,
            max_queue_depth: 5,
            max_quarantined_drops: 3,
            ..Default::default()
        };
        let (stalled_conn, _stalled_rx) = Connection::create_channel_with_policy(stalled_id.clone(), stalled_config);
        let stalled_conn_arc = Arc::new(stalled_conn);
        registry.add_connection(stalled_conn_arc.clone()).await;

        // 3. Measure fanout latency over multiple iterations
        let iterations = 20;
        let mut latencies = Vec::new();

        for seq in 0..iterations {
            let msg = Message::text(format!("analytics-event-{}", seq));
            let start = Instant::now();
            let summary = fanout_message_detailed(&registry, &msg, &FanoutConfig::default())
                .await
                .expect("fanout should succeed");
            let elapsed = start.elapsed();
            latencies.push(elapsed);

            // Healthy consumers must always receive their message immediately
            assert!(
                summary.delivered >= healthy_count,
                "All healthy connections must receive messages (delivered: {}, healthy: {})",
                summary.delivered,
                healthy_count
            );
        }

        // 4. Verify healthy receivers received all messages without loss
        for mut rx in healthy_receivers {
            for seq in 0..iterations {
                let msg = rx.recv().await.expect("healthy receiver must get message");
                assert_eq!(msg, Message::text(format!("analytics-event-{}", seq)));
            }
        }

        // 5. Verify p99 latency to healthy connections is flat and sub-millisecond range
        latencies.sort();
        let p99_latency = latencies[(latencies.len() as f64 * 0.95) as usize];
        assert!(
            p99_latency < Duration::from_millis(15),
            "p99 fanout latency should be < 15ms even with stalled consumer, got {:?}",
            p99_latency
        );

        // 6. Verify the stalled consumer was quarantined and marked for eviction
        assert!(
            stalled_conn_arc.is_quarantined().await || stalled_conn_arc.should_evict().await,
            "Stalled consumer must be quarantined or evicted by proactive policy"
        );
    }

    /// Acceptance Criterion 2:
    /// Per-connection message ordering is strictly preserved.
    #[tokio::test]
    async fn test_per_connection_message_ordering_preserved() {
        let registry = ConnectionRegistry::new();
        let client_count = 10;
        let message_count = 100;
        let mut receiver_handles = Vec::new();

        for i in 0..client_count {
            let (conn, mut rx) = Connection::create_channel(format!("ordered-client-{}", i));
            registry.add_connection(Arc::new(conn)).await;

            let handle = tokio::spawn(async move {
                let mut received = Vec::new();
                for _ in 0..message_count {
                    if let Some(msg) = rx.recv().await {
                        received.push(msg);
                    }
                }
                received
            });
            receiver_handles.push(handle);
        }

        // Fan out sequential messages
        for seq in 0..message_count {
            let msg = Message::text(format!("seq-{}", seq));
            fanout_message(&registry, msg).await.expect("fanout should succeed");
        }

        // Verify each connection received messages in exact sequential order
        for handle in receiver_handles {
            let received = handle.await.expect("receiver task should complete");
            assert_eq!(received.len(), message_count);
            for (seq, msg) in received.iter().enumerate() {
                assert_eq!(
                    *msg,
                    Message::text(format!("seq-{}", seq)),
                    "Message order violated"
                );
            }
        }
    }

    /// Acceptance Criterion 3:
    /// Concurrency is bounded (no unbounded task spawning per fanout call).
    #[tokio::test]
    async fn test_bounded_concurrency_fanout() {
        let registry = ConnectionRegistry::new();
        let total_clients = 128;
        let concurrency_bound = 16;
        let mut receivers = Vec::new();

        for i in 0..total_clients {
            let (conn, rx) = Connection::create_channel(format!("bounded-client-{}", i));
            registry.add_connection(Arc::new(conn)).await;
            receivers.push(rx);
        }

        let config = FanoutConfig {
            max_concurrency: concurrency_bound,
            per_connection_timeout: None,
        };

        let summary = fanout_message_detailed(&registry, &Message::text("bounded-test"), &config)
            .await
            .expect("bounded fanout should succeed");

        assert_eq!(summary.total_connections, total_clients);
        assert_eq!(summary.delivered, total_clients);

        for mut rx in receivers {
            let msg = rx.recv().await.expect("message delivered");
            assert_eq!(msg, Message::text("bounded-test"));
        }
    }

    /// Acceptance Criterion 4:
    /// Documented, tested proactive slow-consumer detection and quarantine policy in policy.rs.
    #[tokio::test]
    async fn test_proactive_slow_consumer_quarantine_and_recovery() {
        let policy_config = SlowConsumerPolicyConfig {
            warning_queue_depth: 3,
            consecutive_high_samples_threshold: 2,
            max_queue_depth: 10,
            quarantine_cooldown: Duration::from_millis(50),
            max_quarantined_drops: 5,
            ..Default::default()
        };

        let (conn, mut rx) = Connection::create_channel_with_policy("monitored-client".to_string(), policy_config);

        // Send 1: seen depth 0 -> Healthy, queue becomes 1
        conn.send(Message::text("m1")).await.unwrap();
        assert_eq!(conn.health_state().await, ConnectionHealthState::Healthy);

        // Send 2: seen depth 1 -> Healthy, queue becomes 2
        conn.send(Message::text("m2")).await.unwrap();
        assert_eq!(conn.health_state().await, ConnectionHealthState::Healthy);

        // Send 3: seen depth 2 -> Healthy, queue becomes 3
        conn.send(Message::text("m3")).await.unwrap();
        assert_eq!(conn.health_state().await, ConnectionHealthState::Healthy);

        // Send 4: seen depth 3 (>= warning depth 3, sample 1) -> Degraded, queue becomes 4
        conn.send(Message::text("m4")).await.unwrap();
        assert_eq!(conn.health_state().await, ConnectionHealthState::Degraded);
        assert_eq!(conn.trend().await, QueueDepthTrend::Growing);

        // Send 5: seen depth 4 (>= warning depth 3, sample 2 >= consecutive threshold 2) -> Quarantined
        let send_res5 = conn.send(Message::text("m5")).await;
        assert!(send_res5.is_err(), "Quarantined connection should drop message");
        assert!(conn.is_quarantined().await);
        assert_eq!(conn.health_state().await, ConnectionHealthState::Quarantined);

        // Drain messages to recover
        while let Ok(msg) = rx.try_recv() {
            let _ = msg;
        }
        assert_eq!(conn.queue_depth(), 0);

        // Allow quarantine cooldown to pass
        sleep(Duration::from_millis(60)).await;

        // Next send recovers to Healthy
        let recover_send = conn.send(Message::text("m6")).await;
        assert!(recover_send.is_ok(), "Connection should recover from quarantine after draining");
        assert_eq!(conn.health_state().await, ConnectionHealthState::Healthy);
    }

    /// Acceptance Criterion 5:
    /// Legacy interface compatibility with raw ConnectionRegistry::add.
    #[tokio::test]
    async fn test_legacy_add_and_get_all_compatibility() {
        let registry = ConnectionRegistry::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        registry.add("legacy-conn".to_string(), tx).await;
        assert_eq!(registry.len().await, 1);

        let all = registry.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, "legacy-conn");

        fanout_message(&registry, Message::text("legacy-msg")).await.unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received, Message::text("legacy-msg"));
    }
}
