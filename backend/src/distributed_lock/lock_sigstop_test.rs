//! Chaos test: SIGSTOP on lock holder, verify stale writer rejection
//!
//! This test reproduces the exact failure mode that Kleppmann identifies:
//! 1. Process A acquires lock with token 0
//! 2. Process A starts writing (but we pause it first)
//! 3. Process A is SIGSTOP'd mid-job
//! 4. Lock TTL expires (we artificially expire it)
//! 5. Process B acquires lock with token 1
//! 6. Process B writes successfully
//! 7. Process A resumes from pause
//! 8. Process A attempts to write with token 0
//! 9. Assertion: Process A's write is rejected (stale writer)
//!
//! Without fencing tokens, step 8 would succeed and corrupt data.

#[cfg(test)]
mod tests {
    use crate::distributed_lock::{DistributedLock, DistributedLockConfig};
    use crate::distributed_lock::store::{InMemoryLockStore, LockStore};
    use std::sync::Arc;
    use std::time::Duration;

    /// Simulates the stale writer problem using an in-memory store
    /// 
    /// In a real test environment with actual processes, you would:
    /// 1. Use Unix signals: send SIGSTOP to pause, SIGCONT to resume
    /// 2. Run actual separate processes, not threads
    /// 3. Use a real Redis or Postgres backend
    /// 
    /// This version uses shared memory and controlled timing to verify
    /// the fencing token mechanism works correctly.
    #[tokio::test]
    async fn test_fencing_rejects_stale_writer() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 1000, // 1 second, will manually expire
            heartbeat_interval_ms: 500,
            acquisition_timeout_ms: 5000,
            max_write_retries: 0,
        };

        // === PHASE 1: Process A acquires lock ===
        let mut lock_a = DistributedLock::new(
            "snapshot_job".to_string(),
            "instance-a".to_string(),
            store.clone(),
            config.clone(),
        );

        let token_a = lock_a.acquire().await.unwrap();
        assert_eq!(token_a.value, 0);
        log::info!("Process A acquired lock with token {}", token_a.value);

        // === PHASE 2: Process A writes under the lock ===
        store
            .write_with_token("snapshot_job", "snapshot_id", "v1", &token_a)
            .await
            .unwrap();
        log::info!("Process A wrote snapshot v1 with token {}", token_a.value);

        // === PHASE 3: Process A is SIGSTOP'd (simulated by forced expiration) ===
        // In a real test, this would be: kill -STOP <pid>
        // Here we simulate it by manually expiring the lock
        log::info!("Simulating SIGSTOP on Process A (forcing lock expiration)");

        // Clear the lock to simulate expiration
        store.force_release_expired("snapshot_job").await.unwrap();

        // === PHASE 4: Process B acquires the now-expired lock ===
        let mut lock_b = DistributedLock::new(
            "snapshot_job".to_string(),
            "instance-b".to_string(),
            store.clone(),
            config.clone(),
        );

        let token_b = lock_b.acquire().await.unwrap();
        assert_eq!(token_b.value, 1); // Monotonically increased
        log::info!("Process B acquired lock with token {}", token_b.value);

        // === PHASE 5: Process B writes under its lock ===
        store
            .write_with_token("snapshot_job", "snapshot_id", "v2", &token_b)
            .await
            .unwrap();
        log::info!("Process B wrote snapshot v2 with token {}", token_b.value);

        // === PHASE 6: Process A resumes from SIGSTOP (simulated) ===
        // In a real test, this would be: kill -CONT <pid>
        log::info!("Simulating SIGCONT on Process A (resuming stale writer)");

        // === PHASE 7: Process A, still holding token 0, tries to write ===
        let result = store
            .write_with_token("snapshot_job", "snapshot_id", "v3", &token_a)
            .await;

        // === ASSERTION: Stale writer is rejected ===
        assert!(
            result.is_err(),
            "Stale writer should be rejected but succeeded"
        );

        if let Err(e) = result {
            log::info!("✓ Stale writer correctly rejected: {}", e);
            assert!(
                e.to_string().contains("Stale writer"),
                "Error should indicate stale writer: {}",
                e
            );
        }

        log::info!("Test passed: fencing tokens prevent stale writer corruption");
    }

    /// Test heartbeat renewal keeps a healthy holder's lock alive
    /// 
    /// Without heartbeat renewal, a job that takes longer than the TTL
    /// would lose its lock mid-job and allow a second instance to acquire it.
    #[tokio::test]
    async fn test_heartbeat_renewal_keeps_lock_alive() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 500, // 500ms TTL
            heartbeat_interval_ms: 200, // Renew every 200ms
            acquisition_timeout_ms: 5000,
            max_write_retries: 0,
        };

        let mut lock_a = DistributedLock::new(
            "long_job".to_string(),
            "instance-a".to_string(),
            store.clone(),
            config.clone(),
        );

        let token_a = lock_a.acquire().await.unwrap();
        log::info!("Process A acquired lock with token {}", token_a.value);

        // Simulate a long job with periodic heartbeats
        for i in 0..5 {
            tokio::time::sleep(Duration::from_millis(200)).await;

            // Heartbeat renewal
            lock_a.renew().await.unwrap();
            log::info!("Process A renewed lock (heartbeat {})", i + 1);

            // Verify lock is still ours
            let metadata = store.get_lock_metadata("long_job").await.unwrap().unwrap();
            assert_eq!(metadata.holder_id, "instance-a");
            assert_eq!(metadata.fencing_token, token_a.value);
        }

        // If we got here without losing the lock, heartbeat renewal works
        log::info!("✓ Heartbeat renewal successfully kept lock alive for 1 second job");

        lock_a.release().await.unwrap();
    }

    /// Test that releasing a lock allows others to acquire it
    #[tokio::test]
    async fn test_release_allows_reacquisition() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig::default();

        let mut lock_a = DistributedLock::new(
            "resource".to_string(),
            "instance-a".to_string(),
            store.clone(),
            config.clone(),
        );

        let token_a = lock_a.acquire().await.unwrap();
        assert_eq!(token_a.value, 0);

        lock_a.release().await.unwrap();
        log::info!("Process A released lock");

        let mut lock_b = DistributedLock::new(
            "resource".to_string(),
            "instance-b".to_string(),
            store.clone(),
            config.clone(),
        );

        let token_b = lock_b.acquire().await.unwrap();
        assert_eq!(token_b.value, 1); // Monotonically increased

        log::info!("✓ Process B successfully acquired lock after Process A released");

        lock_b.release().await.unwrap();
    }

    /// Test competing acquisition attempts with eventual success
    #[tokio::test]
    async fn test_competing_acquisitions() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 1000,
            heartbeat_interval_ms: 500,
            acquisition_timeout_ms: 500,
            max_write_retries: 0,
        };

        let mut lock_a = DistributedLock::new(
            "contested_resource".to_string(),
            "instance-a".to_string(),
            store.clone(),
            config.clone(),
        );

        // A acquires first
        let token_a = lock_a.acquire().await.unwrap();
        log::info!("Process A acquired lock first");

        // B tries to acquire, should fail (A holds it)
        let mut lock_b = DistributedLock::new(
            "contested_resource".to_string(),
            "instance-b".to_string(),
            store.clone(),
            config.clone(),
        );

        let result = tokio::time::timeout(Duration::from_millis(300), lock_b.acquire()).await;
        assert!(result.is_err() || result.as_ref().is_ok_and(|r| r.is_err()));
        log::info!("Process B correctly could not acquire (A holding)");

        // A releases
        lock_a.release().await.unwrap();
        log::info!("Process A released lock");

        // B now acquires successfully
        let token_b = lock_b.acquire().await.unwrap();
        assert_eq!(token_b.value, 1); // Monotonic
        log::info!("✓ Process B acquired lock after A released");

        lock_b.release().await.unwrap();
    }

    /// Verify monotonic fencing tokens across multiple acquisitions
    #[tokio::test]
    async fn test_monotonic_tokens_across_acquisitions() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 100, // Very short, we'll force expire
            ..Default::default()
        };

        let mut tokens = Vec::new();

        // Acquire and release 5 times
        for i in 0..5 {
            let mut lock = DistributedLock::new(
                "token_sequence".to_string(),
                format!("instance-{}", i),
                store.clone(),
                config.clone(),
            );

            let token = lock.acquire().await.unwrap();
            tokens.push(token.value);
            log::info!("Acquisition {} got token {}", i, token.value);

            lock.release().await.unwrap();
        }

        // Verify strictly increasing
        for i in 1..tokens.len() {
            assert!(
                tokens[i] > tokens[i - 1],
                "Token sequence not monotonic: {:?}",
                tokens
            );
        }

        log::info!("✓ All tokens monotonically increased: {:?}", tokens);
    }
}
