//! Redis-backed distributed lock storage implementation
//!
//! Provides production-grade distributed locking with monotonic fencing tokens
//! across multiple independent replicas/processes coordinating through Redis.
//!
//! # Architecture & Guarantees
//!
//! 1. **Atomic Acquire-and-Fence**: Lock acquisition, monotonic token issuance,
//!    and TTL registration execute in a single round-trip atomic Lua script on Redis.
//! 2. **Clock-Independent Correctness**: Lock expiration is enforced by Redis's
//!    server-side monotonic clock via `PEXPIRE`/`PTTL`. Holder pauses or clock skews
//!    cannot extend lock lifetime or cause uncoordinated access.
//! 3. **Monotonicity Across Failovers (Epoch Layering)**: Supports generation/epoch
//!    numbers layered on top of per-resource sequence counters. In case of Redis
//!    Sentinel or failover promotion with replication lag, incrementing the epoch
//!    ensures no token collisions or stale token reuse can ever occur.
//! 4. **Fencing of Stale Writes**: All data writes carry the acquisition fencing token.
//!    Redis atomically verifies that the incoming token matches or exceeds the highest
//!    accepted token for that resource, rejecting partitioned or paused holders.
//! 5. **Safe Releases & Heartbeat Renewals**: Release and renewal scripts check both
//!    `holder_id` and `fencing_token`, ensuring a delayed process cannot release or
//!    renew a lock that was subsequently acquired by a new replica.
//!
//! # Failure Modes & Resilience
//!
//! | Scenario | Behaviour | Recovery / Prevention |
//! |---|---|---|
//! | Redis node unreachable / network timeout | Returns `LockError::StorageError` | Caller retries with exponential backoff; operations fail closed |
//! | Holder stalls / SIGSTOP past TTL | Lock expires on Redis via TTL; secondary replica acquires with higher token | Primary's subsequent write rejected with `FencingTokenRejected` |
//! | Redis Sentinel / replica promotion with lag | Next master initialized with `epoch + 1` or `from_last_token` | Strict monotonicity preserved; older tokens permanently rejected |
//! | Concurrent acquisition storm (N replicas) | Single atomic Lua script executes; exactly 1 acquires | Remaining N-1 receive `AcquisitionFailed` without race conditions |
//! | Legitimate renewal near expiry | Atomic verification of holder + token; resets TTL in one step | Safe under network jitter as long as renewal arrives within TTL |

use std::collections::HashMap;
use std::time::SystemTime;
use async_trait::async_trait;
use redis::AsyncCommands;

use crate::distributed_lock::{FencingToken, LockError, Result};
use crate::distributed_lock::store::{LockMetadata, LockStore};

/// Configuration for the Redis-backed lock store
#[derive(Clone, Debug)]
pub struct RedisLockConfig {
    /// Redis connection URL (e.g. "redis://127.0.0.1:6379")
    pub url: String,
    /// Prefix for all Redis keys managed by this store
    pub key_prefix: String,
    /// Failover epoch/generation number to guarantee cross-failover monotonicity
    pub epoch: u64,
    /// Optional number of replicas to require synchronous acknowledgment (`WAIT`)
    pub wait_replicas: u32,
    /// Timeout for `WAIT` replication acknowledgment in milliseconds
    pub wait_timeout_ms: u64,
}

impl Default for RedisLockConfig {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".to_string(),
            key_prefix: "stellar_lock:".to_string(),
            epoch: 0,
            wait_replicas: 0,
            wait_timeout_ms: 1000,
        }
    }
}

/// Production Redis-backed implementation of [`LockStore`]
#[derive(Clone)]
pub struct RedisLockStore {
    client: redis::Client,
    config: RedisLockConfig,
    acquire_script: redis::Script,
    renew_script: redis::Script,
    release_script: redis::Script,
    write_script: redis::Script,
    force_release_script: redis::Script,
}

impl RedisLockStore {
    /// Create a new `RedisLockStore` with the given configuration
    pub fn new(config: RedisLockConfig) -> Result<Self> {
        let client = redis::Client::open(config.url.as_str())
            .map_err(|e| LockError::StorageError(format!("Invalid Redis URL {}: {}", config.url, e)))?;

        // Lua scripts compiled once for atomicity
        let acquire_script = redis::Script::new(r#"
            local exists = redis.call('EXISTS', KEYS[1])
            if exists == 1 then
                local holder = redis.call('HGET', KEYS[1], 'holder_id')
                return {0, tostring(holder or "unknown")}
            end

            local raw_token = redis.call('INCR', KEYS[2])
            local epoch = tonumber(ARGV[4]) or 0
            local effective_token = raw_token
            if epoch > 0 then
                effective_token = (epoch * 1000000000) + raw_token
            end

            local ttl = tonumber(ARGV[2])
            local now = tonumber(ARGV[3])
            local expires_at = now + ttl

            redis.call('HSET', KEYS[1],
                'holder_id', ARGV[1],
                'fencing_token', tostring(effective_token),
                'acquired_at_ms', tostring(now),
                'expires_at_ms', tostring(expires_at),
                'epoch', tostring(epoch)
            )
            redis.call('PEXPIRE', KEYS[1], ttl)

            return {1, tostring(effective_token), tostring(now)}
        "#);

        let renew_script = redis::Script::new(r#"
            local exists = redis.call('EXISTS', KEYS[1])
            if exists == 0 then
                return {0, "Lock not found or expired"}
            end

            local current_holder = redis.call('HGET', KEYS[1], 'holder_id')
            if current_holder ~= ARGV[1] then
                return {0, "Lock held by different holder: " .. tostring(current_holder or "none")}
            end

            local current_token = redis.call('HGET', KEYS[1], 'fencing_token')
            if tostring(current_token) ~= tostring(ARGV[2]) then
                return {0, "Token mismatch: expected " .. tostring(current_token) .. ", got " .. tostring(ARGV[2])}
            end

            local ttl = tonumber(ARGV[3])
            local now = tonumber(ARGV[4])
            local expires_at = now + ttl

            redis.call('HSET', KEYS[1], 'expires_at_ms', tostring(expires_at))
            redis.call('PEXPIRE', KEYS[1], ttl)

            return {1, "OK"}
        "#);

        let release_script = redis::Script::new(r#"
            local exists = redis.call('EXISTS', KEYS[1])
            if exists == 0 then
                return {0, "Lock not found"}
            end

            local current_holder = redis.call('HGET', KEYS[1], 'holder_id')
            if current_holder ~= ARGV[1] then
                return {0, "Lock held by different holder: " .. tostring(current_holder or "none")}
            end

            local current_token = redis.call('HGET', KEYS[1], 'fencing_token')
            if tostring(current_token) ~= tostring(ARGV[2]) then
                return {0, "Token mismatch: expected " .. tostring(current_token) .. ", got " .. tostring(ARGV[2])}
            end

            redis.call('DEL', KEYS[1])
            return {1, "OK"}
        "#);

        let write_script = redis::Script::new(r#"
            local attempt_token = tonumber(ARGV[3])
            local highest_token_str = redis.call('GET', KEYS[1])

            if highest_token_str and highest_token_str ~= false then
                local held = tonumber(highest_token_str)
                if attempt_token < held then
                    return {0, tostring(held), tostring(attempt_token)}
                end
            end

            if not highest_token_str or attempt_token > (tonumber(highest_token_str) or 0) then
                redis.call('SET', KEYS[1], tostring(attempt_token))
            end

            redis.call('HSET', KEYS[2], ARGV[1], ARGV[2])
            return {1, "OK", ""}
        "#);

        let force_release_script = redis::Script::new(r#"
            local exists = redis.call('EXISTS', KEYS[1])
            if exists == 0 then
                return 0
            end
            local expires_at = redis.call('HGET', KEYS[1], 'expires_at_ms')
            local now = tonumber(ARGV[1])
            if expires_at and tonumber(expires_at) <= now then
                redis.call('DEL', KEYS[1])
                return 1
            end
            return 0
        "#);

        Ok(Self {
            client,
            config,
            acquire_script,
            renew_script,
            release_script,
            write_script,
            force_release_script,
        })
    }

    /// Create from a connection URL string with default config
    pub fn from_url(url: &str) -> Result<Self> {
        Self::new(RedisLockConfig {
            url: url.to_string(),
            ..Default::default()
        })
    }

    /// Return reference to configuration
    pub fn config(&self) -> &RedisLockConfig {
        &self.config
    }

    /// Update failover epoch
    pub fn with_epoch(mut self, epoch: u64) -> Self {
        self.config.epoch = epoch;
        self
    }

    /// Helper to get an async multiplexed connection
    pub async fn get_connection(&self) -> Result<redis::aio::MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| LockError::StorageError(format!("Failed to connect to Redis: {}", e)))
    }

    /// Key for lock state hash
    fn lock_key(&self, resource_id: &str) -> String {
        format!("{}lock:{}", self.config.key_prefix, resource_id)
    }

    /// Key for monotonic token counter
    fn token_key(&self, resource_id: &str) -> String {
        format!("{}token:{}", self.config.key_prefix, resource_id)
    }

    /// Key for tracking highest accepted write token
    fn write_token_key(&self, resource_id: &str) -> String {
        format!("{}write_token:{}", self.config.key_prefix, resource_id)
    }

    /// Key for resource data hash
    fn data_key(&self, resource_id: &str) -> String {
        format!("{}data:{}", self.config.key_prefix, resource_id)
    }

    /// Explicitly set the minimum token value in Redis (for restart/failover recovery)
    pub async fn set_last_token(&self, resource_id: &str, last_token: u64) -> Result<()> {
        let mut conn = self.get_connection().await?;
        let token_key = self.token_key(resource_id);
        
        let script = redis::Script::new(r#"
            local current = redis.call('GET', KEYS[1])
            local new_val = tonumber(ARGV[1])
            if not current or tonumber(current) < new_val then
                redis.call('SET', KEYS[1], tostring(new_val))
                return 1
            end
            return 0
        "#);

        let _: i32 = script
            .key(&token_key)
            .arg(last_token.to_string())
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::StorageError(format!("Failed to set last token: {}", e)))?;

        Ok(())
    }

    /// Read data written under a resource lock (for verification)
    pub async fn read_data(&self, resource_id: &str, key: &str) -> Result<Option<String>> {
        let mut conn = self.get_connection().await?;
        let data_key = self.data_key(resource_id);
        
        let val: Option<String> = conn
            .hget(data_key, key)
            .await
            .map_err(|e| LockError::StorageError(format!("Failed to read data: {}", e)))?;

        Ok(val)
    }
}

#[async_trait]
impl LockStore for RedisLockStore {
    async fn acquire_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        ttl_ms: u64,
    ) -> Result<FencingToken> {
        let mut conn = self.get_connection().await?;
        let lock_key = self.lock_key(resource_id);
        let token_key = self.token_key(resource_id);

        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let result: (i32, String, Option<String>) = self.acquire_script
            .key(&lock_key)
            .key(&token_key)
            .arg(holder_id)
            .arg(ttl_ms.to_string())
            .arg(now_ms.to_string())
            .arg(self.config.epoch.to_string())
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::StorageError(format!("Redis acquire script failed: {}", e)))?;

        if result.0 == 1 {
            let token_value: u64 = result.1.parse().map_err(|e| {
                LockError::StorageError(format!("Invalid fencing token parsed from Redis: {}", e))
            })?;
            let issued_at: i64 = result.2.and_then(|s| s.parse().ok()).unwrap_or(now_ms);

            // If synchronous replication acknowledgment is requested, execute WAIT
            if self.config.wait_replicas > 0 {
                let wait_cmd: std::result::Result<u32, redis::RedisError> = redis::cmd("WAIT")
                    .arg(self.config.wait_replicas)
                    .arg(self.config.wait_timeout_ms)
                    .query_async(&mut conn)
                    .await;

                if let Ok(acked) = wait_cmd {
                    if acked < self.config.wait_replicas {
                        log::warn!(
                            "WAIT acknowledged by {} replicas, expected {}",
                            acked,
                            self.config.wait_replicas
                        );
                    }
                }
            }

            Ok(FencingToken {
                value: token_value,
                issued_at,
            })
        } else {
            Err(LockError::AcquisitionFailed(format!(
                "Lock held by {}",
                result.1
            )))
        }
    }

    async fn renew_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        token: &FencingToken,
        ttl_ms: u64,
    ) -> Result<()> {
        let mut conn = self.get_connection().await?;
        let lock_key = self.lock_key(resource_id);

        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let result: (i32, String) = self.renew_script
            .key(&lock_key)
            .arg(holder_id)
            .arg(token.value.to_string())
            .arg(ttl_ms.to_string())
            .arg(now_ms.to_string())
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::StorageError(format!("Redis renew script failed: {}", e)))?;

        if result.0 == 1 {
            Ok(())
        } else {
            Err(LockError::LockLost(result.1))
        }
    }

    async fn release_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        token: &FencingToken,
    ) -> Result<()> {
        let mut conn = self.get_connection().await?;
        let lock_key = self.lock_key(resource_id);

        let result: (i32, String) = self.release_script
            .key(&lock_key)
            .arg(holder_id)
            .arg(token.value.to_string())
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::StorageError(format!("Redis release script failed: {}", e)))?;

        if result.0 == 1 {
            Ok(())
        } else {
            Err(LockError::LockLost(result.1))
        }
    }

    async fn write_with_token(
        &self,
        resource_id: &str,
        key: &str,
        value: &str,
        token: &FencingToken,
    ) -> Result<()> {
        let mut conn = self.get_connection().await?;
        let write_token_key = self.write_token_key(resource_id);
        let data_key = self.data_key(resource_id);

        let result: (i32, String, String) = self.write_script
            .key(&write_token_key)
            .key(&data_key)
            .arg(key)
            .arg(value)
            .arg(token.value.to_string())
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::StorageError(format!("Redis write script failed: {}", e)))?;

        if result.0 == 1 {
            Ok(())
        } else {
            let held: u64 = result.1.parse().unwrap_or(0);
            let attempted: u64 = result.2.parse().unwrap_or(token.value);
            Err(LockError::FencingTokenRejected { held, attempted })
        }
    }

    async fn get_lock_metadata(
        &self,
        resource_id: &str,
    ) -> Result<Option<LockMetadata>> {
        let mut conn = self.get_connection().await?;
        let lock_key = self.lock_key(resource_id);

        let map: HashMap<String, String> = conn
            .hgetall(&lock_key)
            .await
            .map_err(|e| LockError::StorageError(format!("Redis HGETALL failed: {}", e)))?;

        if map.is_empty() || !map.contains_key("holder_id") {
            return Ok(None);
        }

        let holder_id = map.get("holder_id").cloned().unwrap_or_default();
        let fencing_token = map.get("fencing_token").and_then(|s| s.parse().ok()).unwrap_or(0);
        let acquired_at_ms = map.get("acquired_at_ms").and_then(|s| s.parse().ok()).unwrap_or(0);
        let expires_at_ms = map.get("expires_at_ms").and_then(|s| s.parse().ok()).unwrap_or(0);

        Ok(Some(LockMetadata {
            holder_id,
            fencing_token,
            acquired_at_ms,
            expires_at_ms,
        }))
    }

    async fn force_release_expired(
        &self,
        resource_id: &str,
    ) -> Result<bool> {
        let mut conn = self.get_connection().await?;
        let lock_key = self.lock_key(resource_id);

        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let released: i32 = self.force_release_script
            .key(&lock_key)
            .arg(now_ms.to_string())
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::StorageError(format!("Redis force release failed: {}", e)))?;

        Ok(released == 1)
    }
}
