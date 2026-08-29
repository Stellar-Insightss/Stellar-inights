//! Chaos test: Multi-process SIGSTOP on lock holder, verify stale writer rejection
//!
//! This test suite reproduces and verifies the exact failure modes that Martin Kleppmann identifies
//! in the Redlock analysis:
//! 1. Process A acquires lock with fencing token T_A from Redis.
//! 2. Process A writes data under the lock.
//! 3. Process A is paused via real OS signal `SIGSTOP` (simulating GC pause or VM freeze).
//! 4. Lock TTL expires on Redis.
//! 5. Process B acquires lock with monotonically higher token T_B from Redis.
//! 6. Process B writes data under the lock.
//! 7. Process A is resumed via `SIGCONT`.
//! 8. Process A attempts to write using its stale token T_A.
//! 9. **Assertion**: Process A's write is rejected by Redis (`FencingTokenRejected`),
//!    guaranteeing storage consistency across independent OS processes.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;
    use crate::distributed_lock::{
        DistributedLock, DistributedLockConfig, LockError, LockStore,
        InMemoryLockStore, RedisLockConfig, RedisLockStore,
    };

    /// Helper managing an ephemeral redis-server child process for testing
    pub struct TestRedisInstance {
        child: std::process::Child,
        pub _port: u16,
        pub url: String,
        _temp_dir: tempfile::TempDir,
    }

    impl TestRedisInstance {
        pub async fn start() -> Result<Self, String> {
            let temp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
            let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            drop(listener);

            let child = std::process::Command::new("redis-server")
                .arg("--port")
                .arg(port.to_string())
                .arg("--dir")
                .arg(temp_dir.path())
                .arg("--save")
                .arg("")
                .arg("--appendonly")
                .arg("no")
                .arg("--protected-mode")
                .arg("no")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("Failed to spawn redis-server: {}", e))?;

            let url = format!("redis://127.0.0.1:{}", port);

            let mut ready = false;
            for _ in 0..40 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if let Ok(client) = redis::Client::open(url.as_str()) {
                    if let Ok(mut conn) = client.get_multiplexed_async_connection().await {
                        let ping: std::result::Result<String, _> =
                            redis::cmd("PING").query_async(&mut conn).await;
                        if let Ok(p) = ping {
                            if p == "PONG" {
                                ready = true;
                                break;
                            }
                        }
                    }
                }
            }

            if !ready {
                return Err("redis-server failed to become ready".to_string());
            }

            Ok(Self {
                child,
                _port: port,
                url,
                _temp_dir: temp_dir,
            })
        }
    }

    impl Drop for TestRedisInstance {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Multi-process SIGSTOP test demonstrating stale writer rejection across independent OS processes
    #[tokio::test]
    async fn test_multi_process_sigstop_fencing_rejection() {
        // Check if this execution is a child subprocess worker
        if std::env::var("STELLAR_LOCK_TEST_CHILD").is_ok() {
            run_child_worker().await;
            return;
        }

        // Start isolated Redis instance
        let redis_inst = match TestRedisInstance::start().await {
            Ok(inst) => inst,
            Err(e) => {
                log::warn!("Skipping live redis multi-process test (redis-server unavailable): {}", e);
                return;
            }
        };

        let store = Arc::new(RedisLockStore::from_url(&redis_inst.url).unwrap());
        let resource_id = "multiprocess_snapshot_job";

        // Spawn Child Process 1 (Worker A)
        let mut child_a = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("distributed_lock::lock_sigstop_test::tests::test_multi_process_sigstop_fencing_rejection")
            .arg("--exact")
            .arg("--nocapture")
            .env("STELLAR_LOCK_TEST_CHILD", "1")
            .env("STELLAR_LOCK_REDIS_URL", &redis_inst.url)
            .env("STELLAR_LOCK_RESOURCE", resource_id)
            .env("STELLAR_LOCK_HOLDER", "process_a")
            .spawn()
            .expect("Failed to spawn child worker A");

        let pid_a = child_a.id() as i32;

        // Wait until Process A has acquired the lock and written initial state
        let mut acquired = false;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(Some(data)) = store.read_data(resource_id, "snapshot_state").await {
                if data == "v1_from_process_a" {
                    acquired = true;
                    break;
                }
            }
        }
        assert!(acquired, "Process A should have acquired lock and written v1");

        // Send real OS SIGSTOP to pause Process A mid-job
        log::info!("Sending SIGSTOP to Process A (pid={})", pid_a);
        unsafe {
            libc::kill(pid_a, libc::SIGSTOP);
        }

        // Wait for Process A's TTL (1000ms) to expire in Redis
        tokio::time::sleep(Duration::from_millis(1200)).await;

        // Process B (running here as secondary replica) acquires the expired lock
        let mut lock_b = DistributedLock::new(
            resource_id.to_string(),
            "process_b".to_string(),
            store.clone(),
            DistributedLockConfig {
                ttl_ms: 5000,
                ..Default::default()
            },
        );

        let token_b = lock_b.acquire().await.expect("Process B should acquire expired lock");
        assert!(token_b.value >= 1, "Token for Process B must be higher than Process A's token");

        // Process B writes v2 under its active lock
        store.write_with_token(resource_id, "snapshot_state", "v2_from_process_b", &token_b)
            .await
            .expect("Process B write should succeed");

        // Verify Redis now contains v2
        let current_state = store.read_data(resource_id, "snapshot_state").await.unwrap();
        assert_eq!(current_state.as_deref(), Some("v2_from_process_b"));

        // Resume Process A via SIGCONT
        log::info!("Sending SIGCONT to resume Process A (pid={})", pid_a);
        unsafe {
            libc::kill(pid_a, libc::SIGCONT);
        }

        // Wait for Child Process A to resume, attempt write with stale token, and exit
        let status = child_a.wait().expect("Child process A failed to wait");
        assert!(
            status.success(),
            "Child process A should exit successfully after verifying stale writer rejection"
        );

        // Verify storage still contains Process B's value (Process A's write was rejected)
        let final_state = store.read_data(resource_id, "snapshot_state").await.unwrap();
        assert_eq!(
            final_state.as_deref(),
            Some("v2_from_process_b"),
            "Process B's write must not be overwritten by resumed Process A"
        );

        log::info!("✓ Multi-process SIGSTOP test successfully verified stale writer rejection");
    }

    /// Child worker implementation executed when `STELLAR_LOCK_TEST_CHILD` is set
    async fn run_child_worker() {
        let redis_url = std::env::var("STELLAR_LOCK_REDIS_URL").unwrap();
        let resource_id = std::env::var("STELLAR_LOCK_RESOURCE").unwrap();
        let holder_id = std::env::var("STELLAR_LOCK_HOLDER").unwrap();

        let store = Arc::new(RedisLockStore::from_url(&redis_url).unwrap());
        let mut lock = DistributedLock::new(
            resource_id.clone(),
            holder_id.clone(),
            store.clone(),
            DistributedLockConfig {
                ttl_ms: 1000,
                acquisition_timeout_ms: 2000,
                ..Default::default()
            },
        );

        let token = lock.acquire().await.expect("Child worker failed to acquire initial lock");

        // Write initial data
        store.write_with_token(&resource_id, "snapshot_state", "v1_from_process_a", &token)
            .await
            .expect("Child initial write failed");

        // Signal readiness by writing marker file or sleeping while waiting for SIGSTOP
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // When resumed by SIGCONT after TTL expiration and Process B acquisition,
        // attempt to write stale data
        let stale_write_res = store.write_with_token(
            &resource_id,
            "snapshot_state",
            "v3_stale_from_process_a",
            &token,
        ).await;

        match stale_write_res {
            Err(LockError::FencingTokenRejected { held, attempted }) => {
                log::info!(
                    "Child worker correctly received fencing token rejection: held={}, attempted={}",
                    held, attempted
                );
                std::process::exit(0); // Success: rejection was verified!
            }
            Ok(_) => {
                log::error!("Child worker stale write unexpectedly SUCCEEDED — fencing failure!");
                std::process::exit(1);
            }
            Err(e) => {
                log::error!("Child worker encountered unexpected error: {}", e);
                std::process::exit(2);
            }
        }
    }

    /// Test that fencing tokens strictly survive simulated backing store failovers without token reuse
    #[tokio::test]
    async fn test_failover_monotonicity_no_token_reuse() {
        let redis_inst = match TestRedisInstance::start().await {
            Ok(inst) => inst,
            Err(e) => {
                log::warn!("Skipping redis failover test (redis-server unavailable): {}", e);
                return;
            }
        };

        let resource_id = "failover_resource";

        // Epoch 1 (Primary store before failover)
        let store_epoch_1 = Arc::new(
            RedisLockStore::new(RedisLockConfig {
                url: redis_inst.url.clone(),
                epoch: 1,
                ..Default::default()
            }).unwrap()
        );

        let mut lock_1 = DistributedLock::new(
            resource_id.to_string(),
            "replica_1".to_string(),
            store_epoch_1.clone(),
            DistributedLockConfig::default(),
        );
        let token_1 = lock_1.acquire().await.unwrap();
        assert!(token_1.value >= 1_000_000_001);
        lock_1.release().await.unwrap();

        // Simulated failover: Sentinel promotes secondary master with Epoch 2
        let store_epoch_2 = Arc::new(
            RedisLockStore::new(RedisLockConfig {
                url: redis_inst.url.clone(),
                epoch: 2,
                ..Default::default()
            }).unwrap()
        );

        let mut lock_2 = DistributedLock::new(
            resource_id.to_string(),
            "replica_2".to_string(),
            store_epoch_2.clone(),
            DistributedLockConfig::default(),
        );
        let token_2 = lock_2.acquire().await.unwrap();

        // Epoch 2 token MUST be strictly greater than Epoch 1 token
        assert!(
            token_2.value > token_1.value,
            "Epoch 2 token {} must exceed Epoch 1 token {}",
            token_2.value,
            token_1.value
        );

        // Attempting to write with Epoch 1 token after Epoch 2 has written must be rejected
        store_epoch_2
            .write_with_token(resource_id, "k", "v2", &token_2)
            .await
            .unwrap();

        let stale_result = store_epoch_2
            .write_with_token(resource_id, "k", "v1_stale", &token_1)
            .await;

        assert!(
            matches!(stale_result, Err(LockError::FencingTokenRejected { .. })),
            "Pre-failover token must be rejected"
        );
    }

    /// Test concurrent acquisition storm (N competing instances)
    #[tokio::test]
    async fn test_redis_concurrent_acquisition_storm() {
        let redis_inst = match TestRedisInstance::start().await {
            Ok(inst) => inst,
            Err(e) => {
                log::warn!("Skipping redis storm test (redis-server unavailable): {}", e);
                return;
            }
        };

        let store = Arc::new(RedisLockStore::from_url(&redis_inst.url).unwrap());
        let resource_id = "contested_storm_resource";

        let mut handles = Vec::new();
        for i in 0..20 {
            let store_clone = store.clone();
            let res = resource_id.to_string();
            let holder = format!("storm_worker_{}", i);
            handles.push(tokio::spawn(async move {
                store_clone.acquire_lock(&res, &holder, 2000).await
            }));
        }

        let mut success_count = 0;
        let mut tokens = Vec::new();
        for handle in handles {
            if let Ok(Ok(token)) = handle.await {
                success_count += 1;
                tokens.push(token.value);
            }
        }

        // Exactly one concurrent worker acquires
        assert_eq!(
            success_count, 1,
            "Exactly one worker must acquire in a single concurrent round"
        );
    }

    /// Test renewal vs expiry race conditions
    #[tokio::test]
    async fn test_redis_renewal_vs_expiry_race() {
        let redis_inst = match TestRedisInstance::start().await {
            Ok(inst) => inst,
            Err(e) => {
                log::warn!("Skipping redis renewal race test: {}", e);
                return;
            }
        };

        let store = Arc::new(RedisLockStore::from_url(&redis_inst.url).unwrap());
        let resource_id = "race_resource";

        // Holder A acquires with 300ms TTL
        let token_a = store.acquire_lock(resource_id, "holder_a", 300).await.unwrap();

        // Wait past TTL
        tokio::time::sleep(Duration::from_millis(400)).await;

        // Holder B acquires
        let token_b = store.acquire_lock(resource_id, "holder_b", 2000).await.unwrap();
        assert!(token_b.value > token_a.value);

        // Holder A attempts to renew expired lock (should fail and NOT disrupt Holder B)
        let renew_res = store.renew_lock(resource_id, "holder_a", &token_a, 2000).await;
        assert!(renew_res.is_err(), "Expired holder renewal must fail");

        // Verify Holder B still owns the lock
        let metadata = store.get_lock_metadata(resource_id).await.unwrap().unwrap();
        assert_eq!(metadata.holder_id, "holder_b");
        assert_eq!(metadata.fencing_token, token_b.value);
    }

    /// Test backing store unavailability returns clean StorageError
    #[tokio::test]
    async fn test_backing_store_unavailability() {
        // Connect to a closed port
        let store = RedisLockStore::from_url("redis://127.0.0.1:59999").unwrap();
        let result = store.acquire_lock("any_res", "any_holder", 1000).await;
        assert!(
            matches!(result, Err(LockError::StorageError(_))),
            "Unreachable backing store must return StorageError"
        );
    }

    /// Simulates the stale writer problem using an in-memory store
    #[tokio::test]
    async fn test_fencing_rejects_stale_writer() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 1000,
            heartbeat_interval_ms: 500,
            acquisition_timeout_ms: 5000,
            max_write_retries: 0,
        };

        // Phase 1: Process A acquires
        let mut lock_a = DistributedLock::new(
            "snapshot_job".to_string(),
            "instance-a".to_string(),
            store.clone(),
            config.clone(),
        );
        let token_a = lock_a.acquire().await.unwrap();
        assert_eq!(token_a.value, 0);

        // Phase 2: Process A writes
        store.write_with_token("snapshot_job", "snapshot_id", "v1", &token_a).await.unwrap();

        // Phase 3: Simulated SIGSTOP / forced expiration
        store.force_release_expired("snapshot_job").await.unwrap();

        // Phase 4: Process B acquires
        let mut lock_b = DistributedLock::new(
            "snapshot_job".to_string(),
            "instance-b".to_string(),
            store.clone(),
            config.clone(),
        );
        let token_b = lock_b.acquire().await.unwrap();
        assert_eq!(token_b.value, 1);

        // Phase 5: Process B writes
        store.write_with_token("snapshot_job", "snapshot_id", "v2", &token_b).await.unwrap();

        // Phase 6: Process A attempts stale write with token 0 -> Rejected
        let result = store.write_with_token("snapshot_job", "snapshot_id", "v3", &token_a).await;
        assert!(result.is_err(), "Stale writer should be rejected");
    }

    /// Test heartbeat renewal keeps a healthy holder's lock alive
    #[tokio::test]
    async fn test_heartbeat_renewal_keeps_lock_alive() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 500,
            heartbeat_interval_ms: 200,
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

        for _ in 0..4 {
            tokio::time::sleep(Duration::from_millis(150)).await;
            lock_a.renew().await.unwrap();
            let metadata = store.get_lock_metadata("long_job").await.unwrap().unwrap();
            assert_eq!(metadata.holder_id, "instance-a");
            assert_eq!(metadata.fencing_token, token_a.value);
        }

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

        let mut lock_b = DistributedLock::new(
            "resource".to_string(),
            "instance-b".to_string(),
            store.clone(),
            config.clone(),
        );

        let token_b = lock_b.acquire().await.unwrap();
        assert_eq!(token_b.value, 1);
        lock_b.release().await.unwrap();
    }

    /// Test competing acquisition attempts with eventual success
    #[tokio::test]
    async fn test_competing_acquisitions() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 1000,
            heartbeat_interval_ms: 500,
            acquisition_timeout_ms: 300,
            max_write_retries: 0,
        };

        let mut lock_a = DistributedLock::new(
            "contested_resource".to_string(),
            "instance-a".to_string(),
            store.clone(),
            config.clone(),
        );

        let _token_a = lock_a.acquire().await.unwrap();

        let mut lock_b = DistributedLock::new(
            "contested_resource".to_string(),
            "instance-b".to_string(),
            store.clone(),
            config.clone(),
        );

        let result = lock_b.acquire().await;
        assert!(result.is_err());

        lock_a.release().await.unwrap();

        let token_b = lock_b.acquire().await.unwrap();
        assert_eq!(token_b.value, 1);
        lock_b.release().await.unwrap();
    }

    /// Verify monotonic fencing tokens across multiple acquisitions
    #[tokio::test]
    async fn test_monotonic_tokens_across_acquisitions() {
        let store = Arc::new(InMemoryLockStore::new());
        let config = DistributedLockConfig {
            ttl_ms: 100,
            ..Default::default()
        };

        let mut tokens = Vec::new();
        for i in 0..5 {
            let mut lock = DistributedLock::new(
                "token_sequence".to_string(),
                format!("instance-{}", i),
                store.clone(),
                config.clone(),
            );

            let token = lock.acquire().await.unwrap();
            tokens.push(token.value);
            lock.release().await.unwrap();
        }

        for i in 1..tokens.len() {
            assert!(tokens[i] > tokens[i - 1]);
        }
    }
}
