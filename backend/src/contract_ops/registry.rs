//! Contract registry — a static catalog of every deployed Soroban contract
//! with its individualised TTL policy.
//!
//! Each entry records the contract address, a human-readable label, and
//! the threshold / extend-to values that the [`TtlManager`](super::ttl_manager::TtlManager)
//! uses when deciding whether to issue an on-chain bump.
//!
//! # Design decisions
//!
//! * **Static, not dynamic.** The registry is built from known contract
//!   addresses at startup. Dynamic discovery (via contract metadata or
//!   ledger scanning) adds complexity that isn't justified for < 10
//!   contracts today. A simple config reload or restart adds a contract.
//!
//! * **Per-contract overrides.** The defaults match the on-chain constants
//!   (`HOT_TTL_EXTEND_TO = 535_680`, `HOT_TTL_THRESHOLD = 100_000`) used
//!   across analytics, token_swap, and multisig. Escrow and stellar_insights
//!   may have different persistence profiles and can override.

use serde::{Deserialize, Serialize};

/// Default HOT_TTL_EXTEND_TO: ~31 days at 5 s ledger pace.
pub const DEFAULT_EXTEND_TO: u32 = 535_680;
/// Default HOT_TTL_THRESHOLD: bump when remaining TTL drops below this.
pub const DEFAULT_THRESHOLD: u32 = 100_000;

/// TTL policy for a single contract.
///
/// Threshold and extend_to are in **ledgers**, matching the on-chain
/// `extend_ttl(threshold, extend_to)` convention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TtlPolicy {
    /// Bump when remaining instance/persistent TTL drops below this many ledgers.
    pub threshold: u32,
    /// Extend TTL to this many ledgers on bump.
    pub extend_to: u32,
}

impl Default for TtlPolicy {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            extend_to: DEFAULT_EXTEND_TO,
        }
    }
}

/// A deployed contract that the [`TtlManager`] monitors and bumps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractEntry {
    /// On-chain contract address (C... strkey).
    pub address: String,
    /// Human-readable label, e.g. "analytics", "escrow", "multisig".
    pub label: String,
    /// Per-contract TTL override. `None` uses [`TtlPolicy::default`].
    pub ttl_policy: Option<TtlPolicy>,
}

impl ContractEntry {
    pub fn new(address: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            label: label.into(),
            ttl_policy: None,
        }
    }

    pub fn with_policy(mut self, policy: TtlPolicy) -> Self {
        self.ttl_policy = Some(policy);
        self
    }

    /// Resolve the effective TTL policy (override or default).
    pub fn effective_policy(&self) -> TtlPolicy {
        self.ttl_policy.clone().unwrap_or_default()
    }
}

/// Registry of all monitored contracts.
///
/// Constructed once at startup and shared (via `Arc`) with the
/// [`TtlManager`](super::ttl_manager::TtlManager).
#[derive(Debug, Clone)]
pub struct ContractRegistry {
    contracts: Vec<ContractEntry>,
}

impl ContractRegistry {
    /// Empty registry. Use [`push`](Self::push) or [`from_entries`] to populate.
    pub fn new() -> Self {
        Self {
            contracts: Vec::new(),
        }
    }

    /// Create a registry from a pre-built list of entries.
    pub fn from_entries(entries: Vec<ContractEntry>) -> Self {
        Self { contracts: entries }
    }

    /// Add a contract entry.
    pub fn push(&mut self, entry: ContractEntry) {
        self.contracts.push(entry);
    }

    /// All registered contracts.
    pub fn entries(&self) -> &[ContractEntry] {
        &self.contracts
    }

    /// Look up a contract by label (case-insensitive).
    pub fn find_by_label(&self, label: &str) -> Option<&ContractEntry> {
        let lower = label.to_ascii_lowercase();
        self.contracts.iter().find(|c| c.label.to_ascii_lowercase() == lower)
    }

    /// Look up a contract by address.
    pub fn find_by_address(&self, address: &str) -> Option<&ContractEntry> {
        self.contracts.iter().find(|c| c.address == address)
    }

    /// Number of registered contracts.
    pub fn len(&self) -> usize {
        self.contracts.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }
}

impl Default for ContractRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default registry for the Stellar-Insights platform.
///
/// Addresses come from the deployed contract IDs. When running against
/// testnet or a local validator, callers should construct a custom
/// registry with the appropriate addresses instead.
pub fn default_registry() -> ContractRegistry {
    ContractRegistry::from_entries(vec![
        ContractEntry::new(
            "CA7QYVA3QYK4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "token_contract",
        ),
        ContractEntry::new(
            "CB7QYVA3QYK4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "voting_contract",
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_on_chain_constants() {
        let p = TtlPolicy::default();
        assert_eq!(p.threshold, 100_000);
        assert_eq!(p.extend_to, 535_680);
    }

    #[test]
    fn contract_entry_effective_policy() {
        let entry = ContractEntry::new("CABCDEF", "test");
        assert_eq!(entry.effective_policy(), TtlPolicy::default());

        let custom = TtlPolicy {
            threshold: 50_000,
            extend_to: 200_000,
        };
        let entry = ContractEntry::new("CABCDEF", "test").with_policy(custom.clone());
        assert_eq!(entry.effective_policy(), custom);
    }

    #[test]
    fn registry_find_by_label_case_insensitive() {
        let mut reg = ContractRegistry::new();
        reg.push(ContractEntry::new("C1", "Analytics"));
        reg.push(ContractEntry::new("C2", "Escrow"));

        assert!(reg.find_by_label("analytics").is_some());
        assert!(reg.find_by_label("ANALYTICS").is_some());
        assert!(reg.find_by_label("escrow").is_some());
        assert!(reg.find_by_label("nonexistent").is_none());
    }

    #[test]
    fn registry_find_by_address() {
        let mut reg = ContractRegistry::new();
        reg.push(ContractEntry::new("CABCDEF", "test"));
        assert!(reg.find_by_address("CABCDEF").is_some());
        assert!(reg.find_by_address("CXXXXXX").is_none());
    }

    #[test]
    fn registry_len_and_empty() {
        let mut reg = ContractRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);

        reg.push(ContractEntry::new("C1", "a"));
        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn default_registry_has_two_entries() {
        let reg = default_registry();
        assert_eq!(reg.len(), 2);
    }
}
