#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, vec, Address, Bytes, Env, Symbol,
};

use multisig::{MultisigContract, MultisigContractClient};

// ---------------------------------------------------------------------------
// No-op stub contract for invoke_contract targets
// ---------------------------------------------------------------------------

#[contract]
pub struct NoopContract;

#[contractimpl]
impl NoopContract {
    pub fn noop(_env: Env) {}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup(
    env: &Env,
) -> (
    MultisigContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let owner_a = Address::generate(env);
    let owner_b = Address::generate(env);
    let owner_c = Address::generate(env);

    let owners = vec![env, owner_a.clone(), owner_b.clone(), owner_c.clone()];
    client.initialize(&admin, &owners, &2);

    (client, admin, owner_a, owner_b, owner_c)
}

fn register_noop(env: &Env) -> Address {
    env.register(NoopContract, ())
}

fn empty_args(env: &Env) -> Bytes {
    Bytes::new(env)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A proposer auto-approves; a second owner approves; execute succeeds
/// because snapshotted threshold == 2.
#[test]
fn basic_proposal_reaches_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, owner_a, owner_b, _owner_c) = setup(&env);

    let target = register_noop(&env);
    let id = client.propose(
        &owner_a,
        &target,
        &Symbol::new(&env, "noop"),
        &empty_args(&env),
    );

    assert_eq!(client.approve(&owner_b, &id), 2);
    client.execute(&id);
    assert!(client.get_proposal(&id).executed);
}

/// Reconfiguring after a proposal is open must NOT change who can satisfy
/// the snapshotted threshold on that proposal.
#[test]
fn reconfigure_does_not_affect_open_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, owner_a, owner_b, _owner_c) = setup(&env);

    let target = register_noop(&env);
    let id = client.propose(
        &owner_a,
        &target,
        &Symbol::new(&env, "noop"),
        &empty_args(&env),
    );

    // Reconfigure: remove owner_b, add a brand-new owner, keep threshold 2.
    let new_owner = Address::generate(&env);
    let new_owners = vec![&env, owner_a.clone(), new_owner.clone()];
    client.reconfigure(&new_owners, &1);

    // owner_b is no longer in the live config, but IS in the snapshot.
    let count = client.approve(&owner_b, &id);
    assert_eq!(count, 2);

    // new_owner was NOT in the snapshot → approval must be rejected.
    let result = client.try_approve(&new_owner, &id);
    assert!(result.is_err());

    // Snapshot threshold = 2 → execute now succeeds.
    client.execute(&id);
    assert!(client.get_proposal(&id).executed);

    // Live config changed independently.
    let cfg = client.get_config();
    assert_eq!(cfg.threshold, 1);
    drop(admin);
}

/// A non-owner cannot create a proposal.
#[test]
fn non_owner_cannot_propose() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _owner_a, _owner_b, _owner_c) = setup(&env);
    let outsider = Address::generate(&env);
    let target = register_noop(&env);
    let result = client.try_propose(
        &outsider,
        &target,
        &Symbol::new(&env, "noop"),
        &empty_args(&env),
    );
    assert!(result.is_err());
}

/// Executing before threshold is met must fail.
#[test]
fn execute_before_threshold_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, owner_a, _owner_b, _owner_c) = setup(&env);
    let target = register_noop(&env);
    let id = client.propose(
        &owner_a,
        &target,
        &Symbol::new(&env, "noop"),
        &empty_args(&env),
    );
    // Only 1 approval (threshold = 2).
    let result = client.try_execute(&id);
    assert!(result.is_err());
}

/// Duplicate approval from the same address must be rejected.
#[test]
fn duplicate_approval_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, owner_a, _owner_b, _owner_c) = setup(&env);
    let target = register_noop(&env);
    let id = client.propose(
        &owner_a,
        &target,
        &Symbol::new(&env, "noop"),
        &empty_args(&env),
    );
    let result = client.try_approve(&owner_a, &id);
    assert!(result.is_err());
}

/// Threshold change after proposal creation has no effect on that proposal.
#[test]
fn threshold_snapshot_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, owner_a, owner_b, owner_c) = setup(&env);

    let target = register_noop(&env);
    // Propose with threshold=2 snapshotted.
    let id = client.propose(
        &owner_a,
        &target,
        &Symbol::new(&env, "noop"),
        &empty_args(&env),
    );

    // Raise threshold to 3 via reconfigure.
    let owners = vec![&env, owner_a.clone(), owner_b.clone(), owner_c.clone()];
    client.reconfigure(&owners, &3);

    // Only get 2 approvals (owner_a auto-approved in propose, +owner_b here).
    client.approve(&owner_b, &id);

    // Snapshot threshold was 2, so execute must succeed despite live threshold=3.
    client.execute(&id);
    assert!(client.get_proposal(&id).executed);
}
