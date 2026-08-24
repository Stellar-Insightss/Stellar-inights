#![cfg(feature = "testutils")]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Vec};
use upgrade::{Error, UpgradeManager, UpgradeManagerArgs, UpgradeManagerClient};

fn wasm_hash(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

#[test]
fn governance_gate_and_approval_invariants_are_enforced() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let approver = Address::generate(&env);
    let second_approver = Address::generate(&env);
    let other = Address::generate(&env);
    let approvers = Vec::from_array(&env, [approver.clone(), second_approver]);
    let manager_args = UpgradeManagerArgs::__constructor(&governance, &approvers, &2);
    env.mock_all_auths_allowing_non_root_auth();
    let manager_id = env.register(UpgradeManager, manager_args);
    env.set_auths(&[]);
    let manager = UpgradeManagerClient::new(&env, &manager_id);

    let target = Address::generate(&env);
    let proposal_id =
        manager
            .mock_all_auths()
            .create_proposal(&target, &wasm_hash(&env, 1), &1, &2);
    let evidence = wasm_hash(&env, 9);
    manager
        .mock_all_auths()
        .record_realistic_storage_test(&proposal_id, &evidence);

    if let Err(Ok(error)) = manager
        .mock_all_auths()
        .try_approve_upgrade(&proposal_id, &other)
    {
        assert_eq!(error, Error::Unauthorized);
    } else {
        panic!("unauthorized approver was accepted");
    }

    if let Err(Ok(error)) = manager.try_execute_upgrade(&proposal_id) {
        assert_eq!(error, Error::ThresholdNotReached);
    } else {
        panic!("execution before threshold was accepted");
    }

    manager
        .mock_all_auths()
        .approve_upgrade(&proposal_id, &approver);
    if let Err(Ok(error)) = manager
        .mock_all_auths()
        .try_approve_upgrade(&proposal_id, &approver)
    {
        assert_eq!(error, Error::DuplicateApproval);
    } else {
        panic!("duplicate approval was accepted");
    }
}
