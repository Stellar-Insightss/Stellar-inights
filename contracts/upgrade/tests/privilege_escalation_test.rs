#![cfg(feature = "testutils")]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Vec};
use upgrade::{Error, UpgradeManager, UpgradeManagerArgs, UpgradeManagerClient};

fn dummy_wasm_hash(env: &Env, val: u8) -> BytesN<32> {
    BytesN::from_array(env, &[val; 32])
}

fn setup_manager(env: &Env) -> (Address, Address, Address, Address, UpgradeManagerClient<'static>) {
    let governance = Address::generate(env);
    let approver1 = Address::generate(env);
    let approver2 = Address::generate(env);
    let approvers = Vec::from_array(env, [approver1.clone(), approver2.clone()]);
    let manager_args = UpgradeManagerArgs::__constructor(&governance, &approvers, &2);

    env.mock_all_auths_allowing_non_root_auth();
    let manager_id = env.register(UpgradeManager, manager_args);
    env.set_auths(&[]);

    let manager_client = UpgradeManagerClient::new(env, &manager_id);
    (governance, approver1, approver2, manager_id, manager_client)
}

#[test]
fn test_legitimate_target_upgrade_creation_succeeds() {
    let env = Env::default();
    let (_governance, _approver1, _approver2, _manager_id, manager) = setup_manager(&env);

    let valid_target = Address::generate(&env);
    let proposal_id = manager
        .mock_all_auths()
        .create_proposal(&valid_target, &dummy_wasm_hash(&env, 1), &1, &2);

    let proposal = manager.get_proposal(&proposal_id);
    assert_eq!(proposal.target, valid_target);
    assert_eq!(proposal.id, 1);
}

#[test]
fn test_self_upgrade_escalation_is_blocked() {
    let env = Env::default();
    let (_governance, _approver1, _approver2, manager_id, manager) = setup_manager(&env);

    // Attempt to propose upgrading the UpgradeManager itself
    let result = manager
        .mock_all_auths()
        .try_create_proposal(&manager_id, &dummy_wasm_hash(&env, 0xAA), &1, &2);

    match result {
        Err(Ok(error)) => assert_eq!(error, Error::TargetOutOfScope),
        other => panic!("expected TargetOutOfScope error, got {:?}", other),
    }
}

#[test]
fn test_governance_upgrade_escalation_is_blocked() {
    let env = Env::default();
    let (governance, _approver1, _approver2, _manager_id, manager) = setup_manager(&env);

    // Attempt to propose upgrading the Governance contract address
    let result = manager
        .mock_all_auths()
        .try_create_proposal(&governance, &dummy_wasm_hash(&env, 0xBB), &1, &2);

    match result {
        Err(Ok(error)) => assert_eq!(error, Error::TargetOutOfScope),
        other => panic!("expected TargetOutOfScope error, got {:?}", other),
    }
}

#[test]
fn test_execution_blocked_before_threshold_reached() {
    let env = Env::default();
    let (_governance, approver1, _approver2, _manager_id, manager) = setup_manager(&env);

    let valid_target = Address::generate(&env);
    let proposal_id = manager
        .mock_all_auths()
        .create_proposal(&valid_target, &dummy_wasm_hash(&env, 2), &1, &2);

    let evidence = dummy_wasm_hash(&env, 9);
    manager
        .mock_all_auths()
        .record_realistic_storage_test(&proposal_id, &evidence);

    // Approve once (threshold is 2)
    manager
        .mock_all_auths()
        .approve_upgrade(&proposal_id, &approver1);

    // Execution must be rejected because threshold (2) is not met
    let exec_result = manager.try_execute_upgrade(&proposal_id);
    match exec_result {
        Err(Ok(error)) => assert_eq!(error, Error::ThresholdNotReached),
        other => panic!("expected ThresholdNotReached error, got {:?}", other),
    }
}
