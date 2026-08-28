use soroban_sdk::{Address, Env};

use crate::{Error, GovernanceConfig};

pub struct TargetScope;

impl TargetScope {
    /// Validates whether a target contract address is eligible for upgrade proposals.
    ///
    /// # Restrictions
    /// - An upgrade proposal MUST NOT target the Governance contract (`config.governance`).
    /// - An upgrade proposal MUST NOT target the UpgradeManager contract itself (`env.current_contract_address()`).
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

    /// Returns `true` if the target address matches any restricted contract address.
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
