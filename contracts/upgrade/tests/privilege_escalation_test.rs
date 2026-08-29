#![cfg(feature = "testutils")]

mod test_support;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Vec};
use test_support::{
    expect_manager_error, expect_target_error, fixture_wasm, hash, sign, signed_payload,
    signing_key, ProductionClient, TargetError, V2Client,
};
use upgrade::{
    Error as ManagerError, ProposalStatus, UpgradeManager, UpgradeManagerArgs, UpgradeManagerClient,
};

fn dummy_wasm_hash(env: &Env, val: u8) -> BytesN<32> {
    BytesN::from_array(env, &[val; 32])
}

fn setup_manager(
    env: &Env,
) -> (
    Address,
    Address,
    Address,
    Address,
    UpgradeManagerClient<'static>,
) {
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
    let proposal_id =
        manager
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
    let result = manager.mock_all_auths().try_create_proposal(
        &manager_id,
        &dummy_wasm_hash(&env, 0xAA),
        &1,
        &2,
    );

    match result {
        Err(Ok(error)) => assert_eq!(error, ManagerError::TargetOutOfScope),
        other => panic!("expected TargetOutOfScope error, got {:?}", other),
    }
}

#[test]
fn test_governance_upgrade_escalation_is_blocked() {
    let env = Env::default();
    let (governance, _approver1, _approver2, _manager_id, manager) = setup_manager(&env);

    // Attempt to propose upgrading the Governance contract address
    let result = manager.mock_all_auths().try_create_proposal(
        &governance,
        &dummy_wasm_hash(&env, 0xBB),
        &1,
        &2,
    );

    match result {
        Err(Ok(error)) => assert_eq!(error, ManagerError::TargetOutOfScope),
        other => panic!("expected TargetOutOfScope error, got {:?}", other),
    }
}

#[test]
fn test_execution_blocked_before_threshold_reached() {
    let env = Env::default();
    let (_governance, approver1, _approver2, _manager_id, manager) = setup_manager(&env);

    let valid_target = Address::generate(&env);
    let proposal_id =
        manager
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
        Err(Ok(error)) => assert_eq!(error, ManagerError::ThresholdNotReached),
        other => panic!("expected ThresholdNotReached error, got {:?}", other),
    }
}

#[test]
fn test_governed_target_upgrade_anti_redirection_and_exploit_rejection() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let approver_one = Address::generate(&env);
    let approver_two = Address::generate(&env);
    let approvers = Vec::from_array(&env, [approver_one.clone(), approver_two.clone()]);

    let manager_wasm = fixture_wasm("upgrade");
    let manager_args = UpgradeManagerArgs::__constructor(&governance, &approvers, &2);
    env.mock_all_auths_allowing_non_root_auth();
    let manager_id = env.register(manager_wasm.as_slice(), manager_args);
    env.set_auths(&[]);
    let manager = UpgradeManagerClient::new(&env, &manager_id);

    // Deploy and bootstrap real StellarInsights v1 target
    let admin = Address::generate(&env);
    let (signing_public_key, signing_key) = signing_key(&env);
    let v1_wasm = fixture_wasm("stellar_insights");
    let target_id = env.register(v1_wasm.as_slice(), ());
    let v1 = ProductionClient::new(&env, &target_id);

    v1.mock_all_auths().initialize(&admin, &signing_public_key);
    v1.mock_all_auths().set_upgrade_manager(&manager_id);

    // Establish legitimate pre-upgrade protocol state
    let snapshot_hash = hash(&env, 1);
    let source_data_hash = hash(&env, 11);
    let payload = signed_payload(&env, 1, &snapshot_hash, &source_data_hash);
    let signature = sign(&env, &signing_key, &payload);
    v1.mock_all_auths()
        .submit_snapshot(&1, &snapshot_hash, &source_data_hash, &signature, &admin);
    assert_eq!(v1.latest_epoch(), 1);

    // Upload candidate V2 wasm and execute governed upgrade
    let v2_wasm = fixture_wasm("stellar_insights_v2");
    let v2_hash = env.deployer().upload_contract_wasm(v2_wasm.as_slice());

    let proposal_id = manager
        .mock_all_auths()
        .create_proposal(&target_id, &v2_hash, &1, &2);
    let evidence = hash(&env, 42);
    manager
        .mock_all_auths()
        .record_realistic_storage_test(&proposal_id, &evidence);
    manager
        .mock_all_auths()
        .approve_upgrade(&proposal_id, &approver_one);
    manager
        .mock_all_auths()
        .approve_upgrade(&proposal_id, &approver_two);

    assert_eq!(
        manager.get_proposal(&proposal_id).status,
        ProposalStatus::Approved
    );

    // Execute the upgrade via the real manager flow
    manager.execute_upgrade(&proposal_id);
    assert_eq!(
        manager.get_proposal(&proposal_id).status,
        ProposalStatus::UpgradeApplied
    );

    // Bind V2 client to the upgraded target instance
    let v2 = V2Client::new(&env, &target_id);

    // EXPLOIT ATTEMPT 1: Upgraded target cannot redirect upgrade authority to a rogue manager
    let rogue_manager = Address::generate(&env);
    expect_target_error(
        v2.mock_all_auths().try_set_upgrade_manager(&rogue_manager),
        TargetError::UpgradeManagerAlreadySet,
    );

    // EXPLOIT ATTEMPT 2: Direct unauthenticated or rogue caller cannot invoke governance_upgrade
    let rogue_wasm_hash = hash(&env, 0xEE);
    match v2.try_governance_upgrade(&rogue_wasm_hash, &2, &3) {
        Err(Err(soroban_sdk::InvokeError::Abort)) => {}
        other => panic!("unauthorized direct governance_upgrade was not aborted: {other:?}"),
    }

    // EXPLOIT ATTEMPT 3: Direct unauthorized migrate_schema is rejected
    match v2.try_migrate_schema(&1, &2) {
        Err(Err(soroban_sdk::InvokeError::Abort)) => {}
        other => panic!("unauthorized direct migrate_schema was not aborted: {other:?}"),
    }

    // Legitimate migration succeeds via UpgradeManager
    manager.migrate_upgrade(&proposal_id);
    assert_eq!(
        manager.get_proposal(&proposal_id).status,
        ProposalStatus::Executed
    );

    // Target continues operating safely under V2 schema
    assert_eq!(v2.latest_epoch(), 1);
    let new_epoch = 2u64;
    let new_snapshot_hash = hash(&env, 2);
    let new_source_data_hash = hash(&env, 12);
    let new_payload = signed_payload(&env, new_epoch, &new_snapshot_hash, &new_source_data_hash);
    let new_signature = sign(&env, &signing_key, &new_payload);
    v2.mock_all_auths().submit_snapshot(
        &new_epoch,
        &new_snapshot_hash,
        &new_source_data_hash,
        &new_signature,
        &admin,
    );
    assert_eq!(v2.latest_epoch(), 2);
}

#[test]
fn test_transitive_governance_takeover_prevention() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let approver_one = Address::generate(&env);
    let approvers = Vec::from_array(&env, [approver_one.clone()]);

    let manager_wasm = fixture_wasm("upgrade");
    let manager_args = UpgradeManagerArgs::__constructor(&governance, &approvers, &1);
    env.mock_all_auths_allowing_non_root_auth();
    let manager_id = env.register(manager_wasm.as_slice(), manager_args);
    env.set_auths(&[]);
    let manager = UpgradeManagerClient::new(&env, &manager_id);

    // An attacker / non-governance entity cannot create upgrade proposals
    let _attacker = Address::generate(&env);
    let valid_target = Address::generate(&env);
    let fake_hash = dummy_wasm_hash(&env, 5);

    // Proposal without governance authorization fails
    match manager.try_create_proposal(&valid_target, &fake_hash, &1, &2) {
        Err(Err(soroban_sdk::InvokeError::Abort)) => {}
        other => panic!("unauthorized create_proposal was not aborted: {other:?}"),
    }

    // Upgrading governance directly is rejected by TargetScope
    expect_manager_error(
        manager
            .mock_all_auths()
            .try_create_proposal(&governance, &fake_hash, &1, &2),
        ManagerError::TargetOutOfScope,
    );

    // Upgrading UpgradeManager itself is rejected by TargetScope
    expect_manager_error(
        manager
            .mock_all_auths()
            .try_create_proposal(&manager_id, &fake_hash, &1, &2),
        ManagerError::TargetOutOfScope,
    );
}
