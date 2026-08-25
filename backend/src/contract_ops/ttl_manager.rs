//! Core TTL monitoring and rent-bumping engine.
//!
//! The [`TtlManager`] runs a periodic poll loop that:
//!
//! 1. Iterates over every [`ContractEntry`] in the [`ContractRegistry`].
//! 2. Queries the Stellar node for the remaining TTL of the contract's
//!    instance and any known persistent storage keys via [`RpcClient`].
//! 3. Compares the remaining TTL against the per-contract
//!    [`TtlPolicy::threshold`].
//! 4. If below threshold, issues an `extendFootprintTtl` transaction
//!    through [`RpcClient`] to push the TTL back to
//!    [`TtlPolicy::extend_to`].
//! 5. Updates Prometheus [`TtlMetrics`] for every contract regardless
//!    of whether a bump was issued.
//!
//! # Failure modes
//!
//! | Scenario | Behaviour |
//! |---|---|
//! | RPC node unreachable | Log error, skip cycle, retry next interval |
//! | Bump tx fails (bad seq, insufficient funds) | Log error, increment `bump_errors`, retry next interval |
//! | Contract archived (restore needed) | Log warning with restore instructions; the manager cannot auto-restore |
//! | Multiple manager instances | Each instance may issue redundant bumps; this is safe because `extend_ttl` is idempotent |

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use log::{error, info, warn};
use tokio::sync::watch;
use tokio::time;

use super::metrics::TtlMetrics;
use super::registry::{ContractEntry, ContractRegistry, TtlPolicy};

// ---------------------------------------------------------------------------
// RPC client trait
// ---------------------------------------------------------------------------

/// Abstraction over Stellar RPC operations needed for TTL management.
///
/// This trait is intentionally minimal — only the two operations the
/// manager needs. Implementations exist for the real Stellar RPC node
/// and for in-memory test doubles.
#[async_trait]
pub trait RpcClient: Send + Sync {
    /// Query the remaining TTL (in ledgers) of a contract's instance data.
    ///
    /// Returns `None` if the contract is not found or has been archived.
    async fn get_instance_ttl(&self, contract_address: &str) -> Result<Option<u32>, RpcError>;

    /// Query the remaining TTL of a persistent storage key for a contract.
    ///
    /// Returns `None` if the key does not exist or is archived.
    async fn get_persistent_ttl(
        &self,
        contract_address: &str,
        storage_key: &str,
    ) -> Result<Option<u32>, RpcError>;

    /// Extend the TTL of a contract's instance footprint.
    ///
    /// Corresponds to the Soroban `extendFootprintTtl` host function.
    /// `extend_to` is the target TTL in ledgers.
    async fn extend_instance_ttl(
        &self,
        contract_address: &str,
        extend_to: u32,
    ) -> Result<(), RpcError>;

    /// Extend the TTL of a specific persistent storage entry.
    async fn extend_persistent_ttl(
        &self,
        contract_address: &str,
        storage_key: &str,
        extend_to: u32,
    ) -> Result<(), RpcError>;
}

/// Errors that can occur during RPC operations.
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("RPC transport error: {0}")]
    Transport(String),

    #[error("Contract not found: {0}")]
    ContractNotFound(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Network / rate-limit error: {0}")]
    Network(String),
}

// ---------------------------------------------------------------------------
// Manager configuration
// ---------------------------------------------------------------------------

/// Configuration for the [`TtlManager`] poll loop.
#[derive(Debug, Clone)]
pub struct TtlManagerConfig {
    /// How often to run the TTL check cycle.
    pub poll_interval: Duration,
    /// If `true`, the manager will also bump persistent keys (not just instance).
    pub bump_persistent_keys: bool,
    /// Known persistent storage keys to monitor per contract label.
    /// Only keys listed here will be checked/bumped.
    pub persistent_keys: std::collections::HashMap<String, Vec<String>>,
}

impl Default for TtlManagerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(300), // 5 minutes
            bump_persistent_keys: true,
            persistent_keys: std::collections::HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// TtlManager
// ---------------------------------------------------------------------------

/// Centralised TTL monitor and rent-bumper.
///
/// Call [`run`](Self::run) to start the poll loop, or
/// [`check_once`](Self::check_once) for a single manual sweep.
pub struct TtlManager {
    registry: Arc<ContractRegistry>,
    rpc: Arc<dyn RpcClient>,
    metrics: TtlMetrics,
    config: TtlManagerConfig,
}

impl TtlManager {
    pub fn new(
        registry: Arc<ContractRegistry>,
        rpc: Arc<dyn RpcClient>,
        metrics: TtlMetrics,
        config: TtlManagerConfig,
    ) -> Self {
        Self {
            registry,
            rpc,
            metrics,
            config,
        }
    }

    /// Run the poll loop until `shutdown` fires.
    ///
    /// The loop is cooperative — it checks `shutdown` between cycles and
    /// will exit cleanly on `SIGTERM` / Ctrl-C.
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) {
        info!(
            "TtlManager starting with {} contracts, poll interval {:?}",
            self.registry.len(),
            self.config.poll_interval
        );

        let mut interval = time::interval(self.config.poll_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.check_once().await;
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("TtlManager shutting down");
                        break;
                    }
                }
            }
        }
    }

    /// Run a single TTL check cycle across all registered contracts.
    pub async fn check_once(&self) {
        let start = Instant::now();
        let contracts = self.registry.entries();

        info!("TTL check cycle starting for {} contracts", contracts.len());

        for entry in contracts {
            if let Err(e) = self.check_contract(entry).await {
                error!("TTL check failed for {}: {}", entry.label, e);
            }
        }

        let elapsed = start.elapsed().as_secs_f64();
        self.metrics.observe_check_duration(elapsed);
        info!("TTL check cycle completed in {:.2}s", elapsed);
    }

    /// Check and optionally bump a single contract.
    async fn check_contract(&self, entry: &ContractEntry) -> Result<(), RpcError> {
        let policy = entry.effective_policy();

        // 1. Check instance TTL
        match self.rpc.get_instance_ttl(&entry.address).await? {
            Some(remaining) => {
                self.metrics.set_remaining(&entry.label, remaining);
                if remaining < policy.threshold {
                    info!(
                        "Contract {} instance TTL {} < threshold {} — bumping to {}",
                        entry.label, remaining, policy.threshold, policy.extend_to
                    );
                    match self.rpc
                        .extend_instance_ttl(&entry.address, policy.extend_to)
                        .await
                    {
                        Ok(()) => {
                            self.metrics.record_bump_success(&entry.label);
                            info!("Contract {} instance TTL bumped successfully", entry.label);
                        }
                        Err(e) => {
                            self.metrics.record_bump_error(&entry.label);
                            error!(
                                "Failed to bump instance TTL for {}: {}",
                                entry.label, e
                            );
                        }
                    }
                } else {
                    self.metrics.record_bump_skipped(&entry.label);
                    info!(
                        "Contract {} instance TTL {} is healthy (threshold {})",
                        entry.label, remaining, policy.threshold
                    );
                }
            }
            None => {
                warn!(
                    "Contract {} instance data not found or archived — \
                     manual restore may be required",
                    entry.label
                );
            }
        }

        // 2. Check persistent keys (if enabled)
        if self.config.bump_persistent_keys {
            if let Some(keys) = self.config.persistent_keys.get(&entry.label) {
                for key in keys {
                    self.check_persistent_key(entry, &policy, key).await?;
                }
            }
        }

        Ok(())
    }

    /// Check and optionally bump a single persistent storage key.
    async fn check_persistent_key(
        &self,
        entry: &ContractEntry,
        policy: &TtlPolicy,
        storage_key: &str,
    ) -> Result<(), RpcError> {
        let key_label = format!("{}:{}", entry.label, storage_key);

        match self.rpc
            .get_persistent_ttl(&entry.address, storage_key)
            .await?
        {
            Some(remaining) => {
                self.metrics.set_remaining(&key_label, remaining);
                if remaining < policy.threshold {
                    info!(
                        "Persistent key {} TTL {} < threshold {} — bumping",
                        key_label, remaining, policy.threshold
                    );
                    match self.rpc
                        .extend_persistent_ttl(
                            &entry.address,
                            storage_key,
                            policy.extend_to,
                        )
                        .await
                    {
                        Ok(()) => {
                            self.metrics.record_bump_success(&key_label);
                            info!("Persistent key {} bumped successfully", key_label);
                        }
                        Err(e) => {
                            self.metrics.record_bump_error(&key_label);
                            error!("Failed to bump persistent key {}: {}", key_label, e);
                        }
                    }
                } else {
                    self.metrics.record_bump_skipped(&key_label);
                }
            }
            None => {
                warn!(
                    "Persistent key {} not found on contract {}",
                    storage_key, entry.label
                );
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// In-memory test double for RpcClient
// ---------------------------------------------------------------------------

/// In-memory mock RPC client for unit testing the TTL manager.
pub struct MockRpcClient {
    /// contract_address -> remaining instance TTL in ledgers.
    pub instance_ttls: std::sync::Mutex<std::collections::HashMap<String, Option<u32>>>,
    /// (contract_address, storage_key) -> remaining persistent TTL.
    pub persistent_ttls:
        std::sync::Mutex<std::collections::HashMap<(String, String), Option<u32>>>,
    /// Record of all extend calls: (address, extend_to).
    pub extend_calls: std::sync::Mutex<Vec<(String, u32)>>,
    /// Record of persistent extend calls: (address, key, extend_to).
    pub extend_persistent_calls:
        std::sync::Mutex<Vec<(String, String, u32)>>,
    /// If set, all RPC calls return this error.
    pub error_override: std::sync::Mutex<Option<RpcError>>,
}

impl MockRpcClient {
    pub fn new() -> Self {
        Self {
            instance_ttls: std::sync::Mutex::new(std::collections::HashMap::new()),
            persistent_ttls: std::sync::Mutex::new(std::collections::HashMap::new()),
            extend_calls: std::sync::Mutex::new(Vec::new()),
            extend_persistent_calls: std::sync::Mutex::new(Vec::new()),
            error_override: std::sync::Mutex::new(None),
        }
    }

    /// Set the instance TTL for a contract.
    pub fn set_instance_ttl(&self, address: &str, ttl: Option<u32>) {
        self.instance_ttls
            .lock()
            .unwrap()
            .insert(address.to_string(), ttl);
    }

    /// Set the persistent TTL for a contract + key.
    pub fn set_persistent_ttl(&self, address: &str, key: &str, ttl: Option<u32>) {
        self.persistent_ttls
            .lock()
            .unwrap()
            .insert((address.to_string(), key.to_string()), ttl);
    }

    /// Force all subsequent calls to return an error.
    pub fn set_error(&self, err: RpcError) {
        *self.error_override.lock().unwrap() = Some(err);
    }

    /// Clear the error override.
    pub fn clear_error(&self) {
        *self.error_override.lock().unwrap() = None;
    }

    /// Get a snapshot of extend calls made.
    pub fn get_extend_calls(&self) -> Vec<(String, u32)> {
        self.extend_calls.lock().unwrap().clone()
    }

    /// Get a snapshot of persistent extend calls made.
    pub fn get_extend_persistent_calls(&self) -> Vec<(String, String, u32)> {
        self.extend_persistent_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl RpcClient for MockRpcClient {
    async fn get_instance_ttl(&self, contract_address: &str) -> Result<Option<u32>, RpcError> {
        if let Some(ref err) = *self.error_override.lock().unwrap() {
            return Err(match err {
                RpcError::Transport(s) => RpcError::Transport(s.clone()),
                RpcError::ContractNotFound(s) => RpcError::ContractNotFound(s.clone()),
                RpcError::TransactionFailed(s) => RpcError::TransactionFailed(s.clone()),
                RpcError::Network(s) => RpcError::Network(s.clone()),
            });
        }
        Ok(self
            .instance_ttls
            .lock()
            .unwrap()
            .get(contract_address)
            .copied()
            .unwrap_or(None))
    }

    async fn get_persistent_ttl(
        &self,
        contract_address: &str,
        storage_key: &str,
    ) -> Result<Option<u32>, RpcError> {
        if let Some(ref err) = *self.error_override.lock().unwrap() {
            return Err(match err {
                RpcError::Transport(s) => RpcError::Transport(s.clone()),
                RpcError::ContractNotFound(s) => RpcError::ContractNotFound(s.clone()),
                RpcError::TransactionFailed(s) => RpcError::TransactionFailed(s.clone()),
                RpcError::Network(s) => RpcError::Network(s.clone()),
            });
        }
        Ok(self
            .persistent_ttls
            .lock()
            .unwrap()
            .get(&(contract_address.to_string(), storage_key.to_string()))
            .copied()
            .unwrap_or(None))
    }

    async fn extend_instance_ttl(
        &self,
        contract_address: &str,
        extend_to: u32,
    ) -> Result<(), RpcError> {
        if let Some(ref err) = *self.error_override.lock().unwrap() {
            return Err(match err {
                RpcError::Transport(s) => RpcError::Transport(s.clone()),
                RpcError::ContractNotFound(s) => RpcError::ContractNotFound(s.clone()),
                RpcError::TransactionFailed(s) => RpcError::TransactionFailed(s.clone()),
                RpcError::Network(s) => RpcError::Network(s.clone()),
            });
        }
        self.extend_calls
            .lock()
            .unwrap()
            .push((contract_address.to_string(), extend_to));
        Ok(())
    }

    async fn extend_persistent_ttl(
        &self,
        contract_address: &str,
        storage_key: &str,
        extend_to: u32,
    ) -> Result<(), RpcError> {
        if let Some(ref err) = *self.error_override.lock().unwrap() {
            return Err(match err {
                RpcError::Transport(s) => RpcError::Transport(s.clone()),
                RpcError::ContractNotFound(s) => RpcError::ContractNotFound(s.clone()),
                RpcError::TransactionFailed(s) => RpcError::TransactionFailed(s.clone()),
                RpcError::Network(s) => RpcError::Network(s.clone()),
            });
        }
        self.extend_persistent_calls
            .lock()
            .unwrap()
            .push((
                contract_address.to_string(),
                storage_key.to_string(),
                extend_to,
            ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::Registry;

    fn setup() -> (Arc<ContractRegistry>, Arc<MockRpcClient>, TtlMetrics) {
        let registry = Arc::new(ContractRegistry::from_entries(vec![
            ContractEntry::new("CAAA", "analytics"),
            ContractEntry::new("CBBB", "escrow"),
        ]));
        let rpc = Arc::new(MockRpcClient::new());
        let metrics = TtlMetrics::register(&Registry::new()).unwrap();
        (registry, rpc, metrics)
    }

    #[tokio::test]
    async fn skips_bump_when_ttl_healthy() {
        let (registry, rpc, metrics) = setup();
        rpc.set_instance_ttl("CAAA", Some(400_000)); // well above threshold
        rpc.set_instance_ttl("CBBB", Some(300_000));

        let manager = TtlManager::new(
            registry,
            rpc.clone(),
            metrics,
            TtlManagerConfig::default(),
        );

        manager.check_once().await;

        // No bumps issued — both are healthy.
        assert!(rpc.get_extend_calls().is_empty());
    }

    #[tokio::test]
    async fn bumps_when_ttl_below_threshold() {
        let (registry, rpc, metrics) = setup();
        rpc.set_instance_ttl("CAAA", Some(50_000)); // below 100k threshold
        rpc.set_instance_ttl("CBBB", Some(400_000));

        let manager = TtlManager::new(
            registry,
            rpc.clone(),
            metrics,
            TtlManagerConfig::default(),
        );

        manager.check_once().await;

        let calls = rpc.get_extend_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "CAAA");
        assert_eq!(calls[0].1, 535_680); // extend_to
    }

    #[tokio::test]
    async fn bumps_both_when_both_below_threshold() {
        let (registry, rpc, metrics) = setup();
        rpc.set_instance_ttl("CAAA", Some(10_000));
        rpc.set_instance_ttl("CBBB", Some(20_000));

        let manager = TtlManager::new(
            registry,
            rpc.clone(),
            metrics,
            TtlManagerConfig::default(),
        );

        manager.check_once().await;

        let calls = rpc.get_extend_calls();
        assert_eq!(calls.len(), 2);
    }

    #[tokio::test]
    async fn handles_missing_contract_gracefully() {
        let (registry, rpc, metrics) = setup();
        // analytics: Some(400k), escrow: not set (archived/missing)
        rpc.set_instance_ttl("CAAA", Some(400_000));

        let manager = TtlManager::new(
            registry,
            rpc.clone(),
            metrics,
            TtlManagerConfig::default(),
        );

        // Should not panic even though escrow returns None.
        manager.check_once().await;

        // analytics is healthy, so no bumps.
        assert!(rpc.get_extend_calls().is_empty());
    }

    #[tokio::test]
    async fn handles_rpc_error_without_panic() {
        let (registry, rpc, metrics) = setup();
        rpc.set_instance_ttl("CAAA", Some(50_000));
        rpc.set_instance_ttl("CBBB", Some(400_000));

        // Make the RPC fail on extend calls.
        rpc.set_error(RpcError::Transport("connection refused".into()));

        let manager = TtlManager::new(
            registry,
            rpc.clone(),
            metrics,
            TtlManagerConfig::default(),
        );

        // Should not panic — errors are logged and metrics updated.
        manager.check_once().await;
    }

    #[tokio::test]
    async fn respects_custom_threshold() {
        let registry = Arc::new(ContractRegistry::from_entries(vec![
            ContractEntry::new(
                "CAAA",
                "analytics",
            )
            .with_policy(TtlPolicy {
                threshold: 200_000, // custom higher threshold
                extend_to: 600_000,
            }),
        ]));
        let rpc = Arc::new(MockRpcClient::new());
        rpc.set_instance_ttl("CAAA", Some(150_000)); // above default 100k but below custom 200k

        let metrics = TtlMetrics::register(&Registry::new()).unwrap();
        let manager = TtlManager::new(
            registry,
            rpc.clone(),
            metrics,
            TtlManagerConfig::default(),
        );

        manager.check_once().await;

        let calls = rpc.get_extend_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, 600_000); // custom extend_to
    }

    #[tokio::test]
    async fn bumps_persistent_keys_when_enabled() {
        let (registry, rpc, metrics) = setup();
        rpc.set_instance_ttl("CAAA", Some(400_000));
        rpc.set_persistent_ttl("CAAA", "PreviousSnapshot", Some(50_000));

        let mut config = TtlManagerConfig::default();
        config.bump_persistent_keys = true;
        config
            .persistent_keys
            .insert("analytics".into(), vec!["PreviousSnapshot".into()]);

        let manager = TtlManager::new(registry, rpc.clone(), metrics, config);

        manager.check_once().await;

        // Instance was healthy (400k), no instance bump.
        assert!(rpc.get_extend_calls().is_empty());
        // Persistent key was below threshold, should bump.
        let p_calls = rpc.get_extend_persistent_calls();
        assert_eq!(p_calls.len(), 1);
        assert_eq!(p_calls[0].0, "CAAA");
        assert_eq!(p_calls[0].1, "PreviousSnapshot");
        assert_eq!(p_calls[0].2, 535_680);
    }

    #[tokio::test]
    async fn skip_persistent_when_disabled() {
        let (registry, rpc, metrics) = setup();
        rpc.set_instance_ttl("CAAA", Some(400_000));
        rpc.set_persistent_ttl("CAAA", "PreviousSnapshot", Some(50_000));

        let mut config = TtlManagerConfig::default();
        config.bump_persistent_keys = false;

        let manager = TtlManager::new(registry, rpc.clone(), metrics, config);

        manager.check_once().await;

        assert!(rpc.get_extend_calls().is_empty());
        assert!(rpc.get_extend_persistent_calls().is_empty());
    }

    #[tokio::test]
    async fn run_exits_on_shutdown() {
        let (registry, rpc, metrics) = setup();
        rpc.set_instance_ttl("CAAA", Some(400_000));
        rpc.set_instance_ttl("CBBB", Some(400_000));

        let mut config = TtlManagerConfig::default();
        config.poll_interval = Duration::from_millis(50);

        let manager = TtlManager::new(registry, rpc, metrics, config);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handle = tokio::spawn(async move {
            manager.run(shutdown_rx).await;
        });

        // Let a couple of cycles run.
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Signal shutdown.
        shutdown_tx.send(true).unwrap();

        // Manager should exit promptly.
        handle.await.unwrap();
    }
}
