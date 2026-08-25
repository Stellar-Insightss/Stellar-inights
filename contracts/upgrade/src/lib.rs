#![no_std]

mod proposal;
pub mod schema_version;

pub use proposal::{GovernanceConfig, ProposalStatus, UpgradeProposal};
use schema_version::validate_transition;
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, panic_with_error, Address,
    BytesN, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidThreshold = 4,
    DuplicateApprover = 5,
    ProposalNotFound = 6,
    InvalidProposal = 7,
    RealisticStorageTestRequired = 8,
    InvalidEvidence = 9,
    DuplicateApproval = 10,
    InvalidStatus = 11,
    ThresholdNotReached = 12,
    InvalidSchemaTransition = 13,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DataKey {
    Config,
    NextProposalId,
    Proposal(u32),
    Approval(u32, Address),
}

/// The manager only relies on this narrow, stable target interface. Direct
/// contract calls are automatically authorized when the target requires auth
/// from this manager address.
#[contractclient(name = "GovernedTargetClient")]
pub trait GovernedTarget {
    fn governance_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), TargetError>;

    fn migrate_schema(
        env: Env,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), TargetError>;
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TargetError {
    NotInitialized = 2,
    Unauthorized = 3,
    SchemaMismatch = 10,
    UpgradeManagerNotSet = 11,
    InvalidSchemaTransition = 12,
}

#[contract]
pub struct UpgradeManager;

#[contractimpl]
impl UpgradeManager {
    pub fn __constructor(env: Env, governance: Address, approvers: Vec<Address>, threshold: u32) {
        governance.require_auth();
        if let Err(error) = validate_approvers(&approvers, threshold) {
            panic_with_error!(&env, error);
        }
        env.storage().instance().set(
            &DataKey::Config,
            &GovernanceConfig {
                governance,
                approvers,
                threshold,
            },
        );
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &0u32);
    }

    pub fn get_config(env: Env) -> Result<GovernanceConfig, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(Error::NotInitialized)
    }

    pub fn create_proposal(
        env: Env,
        target: Address,
        new_wasm_hash: BytesN<32>,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<u32, Error> {
        let config = config(&env)?;
        config.governance.require_auth();
        if is_zero_hash(&new_wasm_hash) {
            return Err(Error::InvalidProposal);
        }
        validate_transition(expected_source_schema, target_schema)?;

        let previous_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextProposalId)
            .ok_or(Error::NotInitialized)?;
        let id = previous_id.checked_add(1).ok_or(Error::InvalidProposal)?;
        let proposal = UpgradeProposal {
            id,
            target,
            new_wasm_hash,
            expected_source_schema,
            target_schema,
            proposer: config.governance,
            status: ProposalStatus::Pending,
            approval_count: 0,
            realistic_test_evidence: None,
        };
        env.storage().instance().set(&DataKey::NextProposalId, &id);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(id), &proposal);
        Ok(id)
    }

    pub fn record_realistic_storage_test(
        env: Env,
        proposal_id: u32,
        evidence: BytesN<32>,
    ) -> Result<(), Error> {
        let mut proposal = proposal(&env, proposal_id)?;
        if proposal.status != ProposalStatus::Pending {
            return Err(Error::InvalidStatus);
        }
        proposal.proposer.require_auth();
        if is_zero_hash(&evidence) {
            return Err(Error::InvalidEvidence);
        }
        if proposal.realistic_test_evidence.is_some() {
            return Err(Error::InvalidEvidence);
        }
        proposal.realistic_test_evidence = Some(evidence);
        store_proposal(&env, &proposal);
        Ok(())
    }

    pub fn approve_upgrade(env: Env, proposal_id: u32, approver: Address) -> Result<(), Error> {
        let mut proposal = proposal(&env, proposal_id)?;
        if proposal.status != ProposalStatus::Pending {
            return Err(Error::InvalidStatus);
        }
        approver.require_auth();
        let config = config(&env)?;
        if !is_approver(&config.approvers, &approver) {
            return Err(Error::Unauthorized);
        }
        if proposal.realistic_test_evidence.is_none() {
            return Err(Error::RealisticStorageTestRequired);
        }
        let approval_key = DataKey::Approval(proposal_id, approver);
        if env.storage().persistent().has(&approval_key) {
            return Err(Error::DuplicateApproval);
        }
        env.storage().persistent().set(&approval_key, &true);
        proposal.approval_count += 1;
        if proposal.approval_count >= config.threshold {
            proposal.status = ProposalStatus::Approved;
        }
        store_proposal(&env, &proposal);
        Ok(())
    }

    pub fn execute_upgrade(env: Env, proposal_id: u32) -> Result<(), Error> {
        let mut proposal = proposal(&env, proposal_id)?;
        if proposal.status == ProposalStatus::Pending {
            return Err(Error::ThresholdNotReached);
        }
        if proposal.status != ProposalStatus::Approved {
            return Err(Error::InvalidStatus);
        }

        let target = GovernedTargetClient::new(&env, &proposal.target);
        target.governance_upgrade(
            &proposal.new_wasm_hash,
            &proposal.expected_source_schema,
            &proposal.target_schema,
        );
        // The target executable is changed only when this invocation commits.
        // A second manager call is therefore required to run the new code's
        // explicit schema migration.
        proposal.status = ProposalStatus::UpgradeApplied;
        store_proposal(&env, &proposal);
        Ok(())
    }

    pub fn migrate_upgrade(env: Env, proposal_id: u32) -> Result<(), Error> {
        let mut proposal = proposal(&env, proposal_id)?;
        if proposal.status != ProposalStatus::UpgradeApplied {
            return Err(Error::InvalidStatus);
        }
        let target = GovernedTargetClient::new(&env, &proposal.target);
        target.migrate_schema(&proposal.expected_source_schema, &proposal.target_schema);
        proposal.status = ProposalStatus::Executed;
        store_proposal(&env, &proposal);
        Ok(())
    }

    pub fn get_proposal(env: Env, proposal_id: u32) -> Result<UpgradeProposal, Error> {
        proposal(&env, proposal_id)
    }
}

fn config(env: &Env) -> Result<GovernanceConfig, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

fn proposal(env: &Env, proposal_id: u32) -> Result<UpgradeProposal, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Proposal(proposal_id))
        .ok_or(Error::ProposalNotFound)
}

fn store_proposal(env: &Env, proposal: &UpgradeProposal) {
    env.storage()
        .persistent()
        .set(&DataKey::Proposal(proposal.id), proposal);
}

fn validate_approvers(approvers: &Vec<Address>, threshold: u32) -> Result<(), Error> {
    if threshold == 0 || threshold > approvers.len() {
        return Err(Error::InvalidThreshold);
    }
    for (index, approver) in approvers.iter().enumerate() {
        if approvers
            .iter()
            .take(index)
            .any(|previous| previous == approver)
        {
            return Err(Error::DuplicateApprover);
        }
    }
    Ok(())
}

fn is_approver(approvers: &Vec<Address>, candidate: &Address) -> bool {
    approvers.iter().any(|approver| approver == *candidate)
}

fn is_zero_hash(hash: &BytesN<32>) -> bool {
    hash.to_array() == [0; 32]
}
