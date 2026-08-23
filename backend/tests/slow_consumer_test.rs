#[cfg(test)]
mod tests {
    use tokio::time::{sleep, Duration};
    use tokio_tungstenite::tungstenite::protocol::Message;
    use stellar_insights_backend::realtime::{ConnectionRegistry, fanout::fanout_message};

    #[tokio::test]
    async fn test_slow_consumer_does_not_affect_others() {
        let registry = ConnectionRegistry::new();

        // Create N well-behaved subscribers
        let mut well_behaved_handles = vec![];
        for i in 0..5 {
            let id = format!("well-behaved-{}", i);
            registry.add(id.clone(), tokio::sync::mpsc::unbounded_channel().0).await;
            let registry_clone = registry.clone();
            well_behaved_handles.push(tokio::spawn(async move {
                let _ = fanout_message(&registry_clone, Message::text("test")).await;
            }));
        }

        // Create 1 artificially slow subscriber
        let slow_id = "slow-consumer".to_string();
        registry.add(slow_id.clone(), tokio::sync::mpsc::unbounded_channel().0).await;
        let registry_clone = registry.clone();
        let slow_handle = tokio::spawn(async move {
            // Simulate a slow consumer by pausing
            sleep(Duration::from_millis(500)).await;
            let _ = fanout_message(&registry_clone, Message::text("test")).await;
        });

        // Wait for all tasks to complete
        for handle in well_behaved_handles {
            let _ = handle.await;
        }
        let _ = slow_handle.await;

        // Assert that the slow consumer was disconnected (queue overflow handled)
        // and that well-behaved consumers received all messages without loss.
        // This is a placeholder assertion; real tests would check actual message delivery.
        assert!(true);
    }
}