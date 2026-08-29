//! Distributed locking module with fencing tokens for mutual exclusion
//! across multiple backend instances.
//!
//! This module provides safe distributed locks under partial failure modes:
//! - Monotonically increasing fencing tokens prevent stale writers
//! - Heartbeat renewal maintains locks for long-running jobs
//! - TTL ensures dead holders are detected promptly
//!
//! See: Martin Kleppmann's Redlock critique for the failure modes this prevents
//! https://martin.kleppmann.com/papers/fencing-tokens.pdf

pub mod fencing;
pub mod redis_store;
pub mod store;

#[cfg(test)]
pub mod lock_sigstop_test;

use std::sync::Arc;
use thiserror::Error;

pub use self::fencing::{FencingToken, FencingTokenGenerator, ResourceFencingState};
pub use self::redis_store::{RedisLockConfig, RedisLockStore};
pub use self::store::{DistributedLockConfig, InMemoryLockStore, LockMetadata, LockStore};

/// Errors that can occur during lock operations
#[derive(Error, Debug)]
pub enum LockError {
    #[error("Failed to acquire lock: {0}")]
    AcquisitionFailed(String),
    
    #[error("Lock expired or lost: {0}")]
    LockLost(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Fencing token rejected (stale writer): held={held}, attempted={attempted}")]
    FencingTokenRejected { held: u64, attempted: u64 },
    
    #[error("Heartbeat renewal failed: {0}")]
    HeartbeatFailed(String),
    
    #[error("Invalid lock state: {0}")]
    InvalidState(String),
}

pub type Result<T> = std::result::Result<T, LockError>;

/// A distributed lock with fencing token support
/// 
/// Guards against stale writers by requiring all writes to carry the fencing token
/// issued at acquisition time. The storage layer rejects any write with a token
/// older than the highest token already seen for that resource.
pub struct DistributedLock {
    resource_id: String,
    store: Arc<dyn LockStore>,
    config: DistributedLockConfig,
    token: Option<FencingToken>,
    holder_id: String,
}

impl DistributedLock {
    /// Create a new distributed lock for the given resource
    pub fn new(
        resource_id: String,
        holder_id: String,
        store: Arc<dyn LockStore>,
        config: DistributedLockConfig,
    ) -> Self {
        Self {
            resource_id,
            store,
            config,
            token: None,
            holder_id,
        }
    }

    /// Acquire the lock, obtaining a fencing token
    /// 
    /// Returns a FencingToken that must accompany all writes performed under
    /// this lock. The token is monotonically increasing across acquisitions.
    pub async fn acquire(&mut self) -> Result<FencingToken> {
        // Attempt to acquire lock with retry logic
        let backoff_ms = 50;
        let mut attempts = 0;
        let max_attempts = (self.config.acquisition_timeout_ms / backoff_ms) as usize;

        loop {
            match self.store.acquire_lock(
                &self.resource_id,
                &self.holder_id,
                self.config.ttl_ms,
            ).await {
                Ok(token) => {
                    self.token = Some(token.clone());
                    log::info!(
                        "Lock acquired for resource={} holder={} token={}",
                        self.resource_id,
                        self.holder_id,
                        token.value
                    );
                    return Ok(token);
                }
                Err(e) if attempts < max_attempts => {
                    attempts += 1;
                    tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                }
                Err(e) => {
                    return Err(LockError::AcquisitionFailed(format!(
                        "Failed to acquire lock after {} attempts: {}",
                        attempts, e
                    )));
                }
            }
        }
    }

    /// Renew the lock's TTL (heartbeat)
    /// 
    /// Should be called periodically during long-running jobs to prevent
    /// the lock from expiring. The fencing token remains unchanged.
    pub async fn renew(&self) -> Result<()> {
        let token = self.token.as_ref()
            .ok_or_else(|| LockError::InvalidState("Lock not acquired".to_string()))?;

        self.store.renew_lock(
            &self.resource_id,
            &self.holder_id,
            token,
            self.config.ttl_ms,
        ).await?;

        log::debug!(
            "Lock renewed for resource={} holder={} token={}",
            self.resource_id,
            self.holder_id,
            token.value
        );

        Ok(())
    }

    /// Get the current fencing token
    pub fn token(&self) -> Option<&FencingToken> {
        self.token.as_ref()
    }

    /// Release the lock
    pub async fn release(&self) -> Result<()> {
        let token = self.token.as_ref()
            .ok_or_else(|| LockError::InvalidState("Lock not acquired".to_string()))?;

        self.store.release_lock(
            &self.resource_id,
            &self.holder_id,
            token,
        ).await?;

        log::info!(
            "Lock released for resource={} holder={} token={}",
            self.resource_id,
            self.holder_id,
            token.value
        );

        Ok(())
    }
}

/// Lock guard that automatically releases the lock when dropped
pub struct LockGuard {
    lock: Option<DistributedLock>,
}

impl LockGuard {
    pub fn new(lock: DistributedLock) -> Self {
        Self { lock: Some(lock) }
    }

    pub fn token(&self) -> Option<&FencingToken> {
        self.lock.as_ref().and_then(|l| l.token.as_ref())
    }

    pub fn resource_id(&self) -> Option<&str> {
        self.lock.as_ref().map(|l| l.resource_id.as_str())
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if let Some(lock) = self.lock.take() {
            // Release is async but we're in sync Drop. Log if it fails but don't panic.
            tokio::spawn(async move {
                if let Err(e) = lock.release().await {
                    log::warn!("Failed to release lock on drop: {}", e);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lock_creation() {
        let config = DistributedLockConfig::default();
        let store = Arc::new(InMemoryLockStore::new());
        let lock = DistributedLock::new(
            "test_res".to_string(),
            "test_holder".to_string(),
            store,
            config,
        );
        assert_eq!(lock.token(), None);
        assert_eq!(lock.resource_id, "test_res");
    }
}
