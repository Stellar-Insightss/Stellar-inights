#![cfg(feature = "testutils")]

mod test_support;

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use test_support::{
    expect_manager_error, expect_target_panic, fixture_wasm, hash, sign, signed_payload,
    signing_key, ProductionClient, TargetError, V2Client,
};
use upgrade::{Error as ManagerError, ProposalStatus, UpgradeManagerArgs, UpgradeManagerClient};

#[test]
fn upgrades_the_same_address_after_realistic_v1_use() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let approver_one = Address::generate(&env);
    let approver_two = Address::generate(&env);
    let admin = Address::generate(&env);
    let (signing_public_key, signing_key) = signing_key(&env);
    let approvers = Vec::from_array(&env, [approver_one.clone(), approver_two.clone()]);

    let manager_wasm = fixture_wasm("upgrade");
    let manager_args = UpgradeManagerArgs::__constructor(&governance, &approvers, &2);
    env.mock_all_auths_allowing_non_root_auth();
    let manager_id = env.register(manager_wasm.as_slice(), manager_args);
    env.set_auths(&[]);
    let manager = UpgradeManagerClient::new(&env, &manager_id);

    let v1_wasm = fixture_wasm("stellar_insights");
    let target_id = env.register(v1_wasm.as_slice(), ());
    let v1 = ProductionClient::new(&env, &target_id);
    v1.mock_all_auths().initialize(&admin, &signing_public_key);
    v1.mock_all_auths().set_upgrade_manager(&manager_id);

    let mut stored_snapshots = std::vec::Vec::new();
    for epoch in 1..=3u64 {
        let snapshot_hash = hash(&env, epoch as u8);
        let source_data_hash = hash(&env, (epoch + 10) as u8);
        let payload = signed_payload(&env, epoch, &snapshot_hash, &source_data_hash);
        let signature = sign(&env, &signing_key, &payload);
        v1.mock_all_auths().submit_snapshot(
            &epoch,
            &snapshot_hash,
            &source_data_hash,
            &signature,
            &admin,
        );
        stored_snapshots.push(v1.get_snapshot(&epoch));
    }
    assert_eq!(v1.latest_epoch(), 3);

    let v2_wasm = fixture_wasm("stellar_insights_v2");
    let v2_hash = env.deployer().upload_contract_wasm(v2_wasm.as_slice());
    let proposal_id = manager
        .mock_all_auths()
        .create_proposal(&target_id, &v2_hash, &1, &2);
    expect_manager_error(
        manager
            .mock_all_auths()
            .try_approve_upgrade(&proposal_id, &approver_one),
        ManagerError::RealisticStorageTestRequired,
    );

    let evidence = hash(&env, 99);
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

    // This call is intentionally made without mocked auth. If the target's
    // manager.require_auth() is removed, it would update the target early and
    // this assertion would fail before the manager executes the proposal.
    match v1.try_governance_upgrade(&v2_hash, &1, &2) {
        Err(Err(soroban_sdk::InvokeError::Abort)) => {}
        other => panic!("direct target upgrade unexpectedly succeeded: {other:?}"),
    }

    manager.execute_upgrade(&proposal_id);
    assert_eq!(target_id, manager.get_proposal(&proposal_id).target);

    // Rebinding the V2 client to the original address proves that the same
    // contract instance now runs V2 while its schema-1 storage is untouched.
    // WASM replacement itself must not silently perform migration.
    let v2 = V2Client::new(&env, &target_id);
    expect_target_panic(v2.try_latest_epoch(), TargetError::SchemaMismatch);

    manager.migrate_upgrade(&proposal_id);
    assert_eq!(
        manager.get_proposal(&proposal_id).status,
        ProposalStatus::Executed
    );

    assert_eq!(v2.latest_epoch(), 3);
    for (epoch, expected) in (1..=3u64).zip(stored_snapshots.iter()) {
        assert_eq!(v2.get_snapshot(&epoch), expected.clone());
    }

    let new_epoch = 4u64;
    let new_snapshot_hash = hash(&env, 4);
    let new_source_data_hash = hash(&env, 14);
    let new_payload = signed_payload(&env, new_epoch, &new_snapshot_hash, &new_source_data_hash);
    let new_signature = sign(&env, &signing_key, &new_payload);
    v2.mock_all_auths().submit_snapshot(
        &new_epoch,
        &new_snapshot_hash,
        &new_source_data_hash,
        &new_signature,
        &admin,
    );
    assert_eq!(v2.latest_epoch(), 4);
    assert_eq!(v2.get_snapshot(&1), stored_snapshots[0]);
    assert_eq!(v2.get_snapshot(&4).epoch, 4);

    expect_manager_error(
        manager.try_execute_upgrade(&proposal_id),
        ManagerError::InvalidStatus,
    );
}
