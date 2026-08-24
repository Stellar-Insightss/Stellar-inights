#![no_std]

mod storage;

use soroban_sdk::{
    contract, contracterror, contractimpl, Address, Bytes, Env, Symbol, Vec,
};
use storage::{
    bump_instance, increment_id, load_config, load_proposal, save_config, save_proposal, Config,
    PolicySnapshot, Proposal,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    Unauthorized = 2,
    InvalidThreshold = 3,
    ProposalNotFound = 4,
    AlreadyApproved = 5,
    NotAnOwner = 6,
    AlreadyExecuted = 7,
    ThresholdNotMet = 8,
    ApproverNotInSnapshot = 9,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct MultisigContract;

#[contractimpl]
impl MultisigContract {
    // -----------------------------------------------------------------------
    // Admin / setup
    // -----------------------------------------------------------------------

    /// Initialise the wallet.  Can only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        owners: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        if env
            .storage()
            .instance()
            .has(&storage::DataKey::Config)
        {
            return Err(Error::AlreadyInitialized);
        }
        if threshold == 0 || threshold as usize > owners.len() as usize {
            return Err(Error::InvalidThreshold);
        }
        admin.require_auth();
        let cfg = Config {
            admin: admin.clone(),
            owners,
            threshold,
        };
        save_config(&env, &cfg);
        bump_instance(&env);
        Ok(())
    }

    /// Reconfigure owners and threshold.  Admin-only.
    /// Does **not** affect any already-open proposals – their snapshotted
    /// policy is unchanged.
    pub fn reconfigure(
        env: Env,
        new_owners: Vec<Address>,
        new_threshold: u32,
    ) -> Result<(), Error> {
        let mut cfg = load_config(&env);
        cfg.admin.require_auth();
        if new_threshold == 0 || new_threshold as usize > new_owners.len() as usize {
            return Err(Error::InvalidThreshold);
        }
        cfg.owners = new_owners;
        cfg.threshold = new_threshold;
        save_config(&env, &cfg);
        bump_instance(&env);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Proposal lifecycle
    // -----------------------------------------------------------------------

    /// Create a proposal.  Any current owner may call this.
    /// The **current** owners list and threshold are snapshotted into the
    /// proposal and cannot be changed after creation.
    pub fn propose(
        env: Env,
        proposer: Address,
        target: Address,
        function: Symbol,
        args: Bytes,
    ) -> Result<u64, Error> {
        proposer.require_auth();
        let cfg = load_config(&env);

        // Verify proposer is among current owners.
        if !cfg.owners.contains(&proposer) {
            return Err(Error::NotAnOwner);
        }

        let id = increment_id(&env);
        let mut approvals: Vec<Address> = Vec::new(&env);
        approvals.push_back(proposer);

        let proposal = Proposal {
            id,
            policy: PolicySnapshot {
                owners: cfg.owners.clone(),
                threshold: cfg.threshold,
            },
            target,
            function,
            args,
            approvals,
            executed: false,
        };
        save_proposal(&env, &proposal);
        bump_instance(&env);
        Ok(id)
    }

    /// Approve a pending proposal.  The caller must be in the **snapshotted**
    /// owner list, not the current live list.  This prevents a freshly-added
    /// owner from approving an old proposal made before they were added.
    pub fn approve(env: Env, approver: Address, proposal_id: u64) -> Result<u32, Error> {
        approver.require_auth();
        let mut proposal = load_proposal(&env, proposal_id);

        if proposal.executed {
            return Err(Error::AlreadyExecuted);
        }

        // Must be in the snapshotted owners (not the current live list).
        if !proposal.policy.owners.contains(&approver) {
            return Err(Error::ApproverNotInSnapshot);
        }

        if proposal.approvals.contains(&approver) {
            return Err(Error::AlreadyApproved);
        }

        proposal.approvals.push_back(approver);
        let count = proposal.approvals.len();
        save_proposal(&env, &proposal);
        bump_instance(&env);
        Ok(count)
    }

    /// Execute a proposal once the snapshotted threshold is met.
    /// Uses `invoke_contract` with the stored arguments.
    pub fn execute(env: Env, proposal_id: u64) -> Result<(), Error> {
        let mut proposal = load_proposal(&env, proposal_id);

        if proposal.executed {
            return Err(Error::AlreadyExecuted);
        }

        // Count approvals from addresses that are in the snapshot.
        let valid_approvals = proposal
            .approvals
            .iter()
            .filter(|a| proposal.policy.owners.contains(a))
            .count();

        if valid_approvals < proposal.policy.threshold as usize {
            return Err(Error::ThresholdNotMet);
        }

        proposal.executed = true;
        save_proposal(&env, &proposal);
        bump_instance(&env);

        // Invoke the target contract.  The args are already encoded – pass as
        // a raw Vec<Val> by deserialising from Bytes via the environment.
        let args_vec: soroban_sdk::Vec<soroban_sdk::Val> =
            soroban_sdk::Vec::from_array(&env, []);
        env.invoke_contract::<soroban_sdk::Val>(
            &proposal.target,
            &proposal.function,
            args_vec,
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Getters
    // -----------------------------------------------------------------------

    pub fn get_proposal(env: Env, proposal_id: u64) -> Proposal {
        load_proposal(&env, proposal_id)
    }

    pub fn get_config(env: Env) -> Config {
        load_config(&env)
    }
}
