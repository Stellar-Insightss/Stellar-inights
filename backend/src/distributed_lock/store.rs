//! Distributed lock storage backends
//!
//! Abstracts over Redis or Postgres advisory locks as the backing store.
//! The storage layer is responsible for:
//! 1. Persisting lock state (holder ID, token, expiration)
//! 2. Enforcing TTL (locks expire if not renewed)
//! 3. Rejecting stale writes (fencing token validation)

use async_trait::async_trait;
use std::time::{Duration, SystemTime};
use crate::distributed_lock::{LockError, Result, FencingToken};
use prometheus::{Counter, Histogram, Registry};

/// Configuration for distributed locks
///
/// Lock TTL and heartbeat intervals must account for the longest-running job:
/// - TTL: Should be ~2-3x longer than the longest expected job duration
///   * Too short: healthy holders lose locks mid-job
///   * Too long: dead holders block new acquisitions for too long
/// - Heartbeat interval: Should fire every 0.5-1.0x of the TTL to maintain healthy locks
///
/// Example: For a 30-second job with 10s variance:
/// - Use TTL = 60s (2x longest expected duration)
/// - Heartbeat interval = 30s (renew before half the TTL expires)
/// - If holder stalls > 60s, secondary instance acquires within 60s
#[derive(Clone, Debug)]
pub struct DistributedLockConfig {
    /// Lock time-to-live in milliseconds
    /// 
    /// Default: 30000ms (30 seconds)
    /// Adjust based on job duration:
    /// - Ingestion sweep: ~15s → TTL=30000ms, heartbeat=15000ms
    /// - Snapshot generation: ~60s → TTL=120000ms, heartbeat=60000ms
    /// - Alert evaluation: ~10s → TTL=20000ms, heartbeat=10000ms
    pub ttl_ms: u64,

    /// Heartbeat renewal interval in milliseconds
    /// 
    /// Default: 15000ms (15 seconds)
    /// Should be roughly TTL/2 to keep healthy holders alive
    pub heartbeat_interval_ms: u64,

    /// Time to wait for lock acquisition before timing out
    /// 
    /// Default: 5000ms (5 seconds)
    pub acquisition_timeout_ms: u64,

    /// Maximum number of write retries with fencing token
    /// 
    /// Default: 3
    /// If a write's fencing token is rejected, retrying won't help (indicates stale writer)
    pub max_write_retries: usize,
}

impl Default for DistributedLockConfig {
    fn default() -> Self {
        Self {
            ttl_ms: 30_000,
            heartbeat_interval_ms: 15_000,
            acquisition_timeout_ms: 5_000,
            max_write_retries: 0, // Don't retry on fencing token rejection
        }
    }
}

/// Lock metadata stored in the backing store
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LockMetadata {
    pub holder_id: String,
    pub fencing_token: u64,
    pub acquired_at_ms: i64,
    pub expires_at_ms: i64,
}

impl LockMetadata {
    pub fn is_expired(&self, now_ms: i64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

/// Storage backend abstraction
/// 
/// Implementors must guarantee:
/// 1. Atomicity: Lock acquisition and token issuance are atomic
/// 2. Monotonicity: Tokens always increase for a given resource
/// 3. Exclusivity: Only one holder can successfully acquire at a time
/// 4. Fencing: Writes with old tokens are rejected
#[async_trait]
pub trait LockStore: Send + Sync {
    /// Acquire a lock for the given resource
    /// 
    /// Must return a new, monotonically increasing fencing token.
    /// If another instance holds the lock, returns an error.
    async fn acquire_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        ttl_ms: u64,
    ) -> Result<FencingToken>;

    /// Renew an existing lock's TTL
    /// 
    /// Must verify the holder still owns the lock before renewing.
    /// Must NOT issue a new token (token stays the same).
    async fn renew_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        token: &FencingToken,
        ttl_ms: u64,
    ) -> Result<()>;

    /// Release a lock
    /// 
    /// Must verify the holder still owns the lock before releasing.
    async fn release_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        token: &FencingToken,
    ) -> Result<()>;

    /// Write data with fencing token validation
    /// 
    /// Must reject writes with tokens older than the highest token
    /// already accepted for this resource.
    async fn write_with_token(
        &self,
        resource_id: &str,
        key: &str,
        value: &str,
        token: &FencingToken,
    ) -> Result<()>;

    /// Read the current lock metadata (for observability/debugging)
    async fn get_lock_metadata(
        &self,
        resource_id: &str,
    ) -> Result<Option<LockMetadata>>;

    /// Force-release an expired lock (cleanup operation)
    async fn force_release_expired(
        &self,
        resource_id: &str,
    ) -> Result<bool>; // true if a lock was actually released
}

/// In-memory lock store implementation (for testing)
pub struct InMemoryLockStore {
    locks: std::sync::Mutex<std::collections::HashMap<String, LockMetadata>>,
    tokens: std::sync::Mutex<std::collections::HashMap<String, u64>>,
    writes: std::sync::Mutex<std::collections::HashMap<String, Vec<(u64, String)>>>,
}

impl InMemoryLockStore {
    pub fn new() -> Self {
        Self {
            locks: std::sync::Mutex::new(std::collections::HashMap::new()),
            tokens: std::sync::Mutex::new(std::collections::HashMap::new()),
            writes: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[async_trait]
impl LockStore for InMemoryLockStore {
    async fn acquire_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        ttl_ms: u64,
    ) -> Result<FencingToken> {
        let mut locks = self.locks.lock().unwrap();
        let mut tokens = self.tokens.lock().unwrap();

        // Check if already locked
        if let Some(existing) = locks.get(resource_id) {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            if !existing.is_expired(now) {
                return Err(LockError::AcquisitionFailed(
                    format!("Lock held by {}", existing.holder_id),
                ));
            }
        }

        // Generate next token
        let next_token = tokens.entry(resource_id.to_string()).or_insert(0);
        let token_value = *next_token;
        *next_token += 1;

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let metadata = LockMetadata {
            holder_id: holder_id.to_string(),
            fencing_token: token_value,
            acquired_at_ms: now,
            expires_at_ms: now + ttl_ms as i64,
        };

        locks.insert(resource_id.to_string(), metadata);

        Ok(FencingToken {
            value: token_value,
            issued_at: now,
        })
    }

    async fn renew_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        token: &FencingToken,
        ttl_ms: u64,
    ) -> Result<()> {
        let mut locks = self.locks.lock().unwrap();

        let metadata = locks
            .get_mut(resource_id)
            .ok_or_else(|| LockError::LockLost("Lock not found".to_string()))?;

        if metadata.holder_id != holder_id {
            return Err(LockError::LockLost(
                format!("Lock held by different holder: {}", metadata.holder_id),
            ));
        }

        if metadata.fencing_token != token.value {
            return Err(LockError::LockLost(
                format!("Token mismatch: expected {}, got {}", metadata.fencing_token, token.value),
            ));
        }

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        metadata.expires_at_ms = now + ttl_ms as i64;

        Ok(())
    }

    async fn release_lock(
        &self,
        resource_id: &str,
        holder_id: &str,
        token: &FencingToken,
    ) -> Result<()> {
        let mut locks = self.locks.lock().unwrap();

        let metadata = locks
            .get(resource_id)
            .ok_or_else(|| LockError::LockLost("Lock not found".to_string()))?;

        if metadata.holder_id != holder_id {
            return Err(LockError::LockLost(
                format!("Lock held by different holder: {}", metadata.holder_id),
            ));
        }

        if metadata.fencing_token != token.value {
            return Err(LockError::LockLost(
                format!("Token mismatch: expected {}, got {}", metadata.fencing_token, token.value),
            ));
        }

        locks.remove(resource_id);

        Ok(())
    }

    async fn write_with_token(
        &self,
        resource_id: &str,
        key: &str,
        value: &str,
        token: &FencingToken,
    ) -> Result<()> {
        let mut writes = self.writes.lock().unwrap();
        let mut highest_token = 0;

        // Check if we've already seen a higher token for this resource
        if let Some(history) = writes.get(resource_id) {
            if let Some((max_token, _)) = history.iter().max_by_key(|(t, _)| t) {
                highest_token = *max_token;
            }
        }

        if token.value < highest_token {
            return Err(LockError::FencingTokenRejected {
                held: highest_token,
                attempted: token.value,
            });
        }

        writes
            .entry(resource_id.to_string())
            .or_insert_with(Vec::new)
            .push((token.value, value.to_string()));

        Ok(())
    }

    async fn get_lock_metadata(
        &self,
        resource_id: &str,
    ) -> Result<Option<LockMetadata>> {
        Ok(self.locks.lock().unwrap().get(resource_id).cloned())
    }

    async fn force_release_expired(
        &self,
        resource_id: &str,
    ) -> Result<bool> {
        let mut locks = self.locks.lock().unwrap();

        if let Some(metadata) = locks.get(resource_id) {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            if metadata.is_expired(now) {
                locks.remove(resource_id);
                return Ok(true);
            }
        }

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store_acquire_release() {
        let store = InMemoryLockStore::new();

        let token = store
            .acquire_lock("resource1", "holder1", 5000)
            .await
            .unwrap();

        assert_eq!(token.value, 0);

        store
            .release_lock("resource1", "holder1", &token)
            .await
            .unwrap();

        let metadata = store.get_lock_metadata("resource1").await.unwrap();
        assert!(metadata.is_none());
    }

    #[tokio::test]
    async fn test_in_memory_store_exclusive() {
        let store = InMemoryLockStore::new();

        let _token1 = store
            .acquire_lock("resource1", "holder1", 5000)
            .await
            .unwrap();

        let result = store
            .acquire_lock("resource1", "holder2", 5000)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_in_memory_store_monotonic_tokens() {
        let store = InMemoryLockStore::new();

        let token1 = store
            .acquire_lock("resource1", "holder1", 1000)
            .await
            .unwrap();

        store
            .release_lock("resource1", "holder1", &token1)
            .await
            .unwrap();

        let token2 = store
            .acquire_lock("resource1", "holder2", 1000)
            .await
            .unwrap();

        assert!(token2.value > token1.value);
    }

    #[tokio::test]
    async fn test_write_with_token_rejects_stale() {
        let store = InMemoryLockStore::new();

        let token1 = store
            .acquire_lock("resource1", "holder1", 5000)
            .await
            .unwrap();

        store
            .write_with_token("resource1", "key1", "value1", &token1)
            .await
            .unwrap();

        store
            .release_lock("resource1", "holder1", &token1)
            .await
            .unwrap();

        let token2 = store
            .acquire_lock("resource1", "holder2", 5000)
            .await
            .unwrap();

        store
            .write_with_token("resource1", "key2", "value2", &token2)
            .await
            .unwrap();

        // Old holder tries to write with stale token
        let result = store
            .write_with_token("resource1", "key3", "value3", &token1)
            .await;

        assert!(result.is_err());
        if let Err(LockError::FencingTokenRejected { held, attempted }) = result {
            assert_eq!(held, 1);
            assert_eq!(attempted, 0);
        } else {
            panic!("Expected FencingTokenRejected");
        }
    }
}
