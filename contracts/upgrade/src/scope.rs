use soroban_sdk::{Address, Env};

use crate::{Error, GovernanceConfig};

/// Formal invariants governing target scope validation and transitive authorization boundaries:
///
/// 1. **Governance Root Isolation Invariant**:
///    $$\forall t \in \text{Address}, t = \text{config.governance} \implies \text{validate\_target\_scope}(t) = \text{Err}(\text{TargetOutOfScope})$$
///    Prevents upgrade proposals from targeting the Governance multi-signature contract. This guarantees that
///    governance rules, threshold configurations, and owner sets cannot be overridden via upgrade proposals.
///
/// 2. **Upgrade Manager Non-Self-Modification Invariant**:
///    $$\forall t \in \text{Address}, t = \text{env.current\_contract\_address}() \implies \text{validate\_target\_scope}(t) = \text{Err}(\text{TargetOutOfScope})$$
///    Prevents `UpgradeManager` from modifying its own executable WASM. This guarantees that proposal lifecycles,
///    approver thresholds, storage test evidence validation, and target scope checks cannot be bypassed or dismantled.
///
/// 3. **Transitive Authority Non-Redirection Invariant**:
///    Governed target contracts (such as `StellarInsights`) bind their `UpgradeManager` reference during initialization
///    as a write-once value (`UpgradeManagerAlreadySet`). Code installed at a governed target cannot reassign or
///    redirect upgrade authority to rogue managers, nor can it bypass the UpgradeManager authentication required for
///    subsequent `governance_upgrade` or `migrate_schema` calls.
///
/// 4. **Non-Delegation of Governance Authority Invariant**:
///    Governed target contracts hold domain-specific capabilities (e.g. snapshot storage) and cannot acquire
///    or delegate governance administrative powers over other contracts in the privilege graph.
pub struct TargetScope;

impl TargetScope {
    /// Validates whether a target contract address is eligible for upgrade proposals.
    ///
    /// # Enforced Invariants
    /// - **Governance Isolation**: Proposal MUST NOT target the Governance contract (`config.governance`).
    /// - **Manager Self-Upgrade Defense**: Proposal MUST NOT target the UpgradeManager contract itself (`env.current_contract_address()`).
    /// - **Transitive Scope Boundary**: Prevents capture or replacement of core authorization infrastructure.
    pub fn validate_target_scope(
        env: &Env,
        target: &Address,
        config: &GovernanceConfig,
    ) -> Result<(), Error> {
        if Self::is_restricted_target(env, target, &config.governance) {
            return Err(Error::TargetOutOfScope);
        }
        Ok(())
    }

    /// Returns `true` if the target address matches any restricted contract address in the privilege graph.
    pub fn is_restricted_target(env: &Env, target: &Address, governance: &Address) -> bool {
        if target == governance {
            return true;
        }
        if target == &env.current_contract_address() {
            return true;
        }
        false
    }
}
