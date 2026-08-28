#![cfg(feature = "testutils")]

mod test_support;

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use test_support::{
    expect_manager_error, expect_target_error, expect_target_panic, fixture_wasm, hash, sign,
    signed_payload, signing_key, LegacyClient, ProductionClient, V2Client,
};
use upgrade::{Error as ManagerError, UpgradeManagerArgs, UpgradeManagerClient};

fn setup(env: &Env, legacy: bool) -> (Address, Address, Address, soroban_sdk::BytesN<32>) {
    let governance = Address::generate(env);
    let approver_one = Address::generate(env);
    let approvers = Vec::from_array(env, [approver_one.clone()]);
    let manager_wasm = fixture_wasm("upgrade");
    let manager_args = UpgradeManagerArgs::__constructor(&governance, &approvers, &1);
    env.mock_all_auths_allowing_non_root_auth();
    let manager_id = env.register(manager_wasm.as_slice(), manager_args);
    env.set_auths(&[]);

    let admin = Address::generate(env);
    let (signing_public_key, signing_key) = signing_key(env);
    let target_wasm = if legacy {
        fixture_wasm("stellar_insights_legacy")
    } else {
        fixture_wasm("stellar_insights")
    };
    let target_id = env.register(target_wasm.as_slice(), ());
    if legacy {
        let target = LegacyClient::new(env, &target_id);
        target
            .mock_all_auths()
            .initialize_legacy(&admin, &signing_public_key);
        target.mock_all_auths().set_upgrade_manager(&manager_id);
        let snapshot_hash = hash(env, 1);
        let source_data_hash = hash(env, 11);
        let payload = signed_payload(env, 1, &snapshot_hash, &source_data_hash);
        let signature = sign(env, &signing_key, &payload);
        target.mock_all_auths().submit_snapshot(
            &1,
            &snapshot_hash,
            &source_data_hash,
            &signature,
            &admin,
        );
    } else {
        let target = ProductionClient::new(env, &target_id);
        target
            .mock_all_auths()
            .initialize(&admin, &signing_public_key);
        target.mock_all_auths().set_upgrade_manager(&manager_id);
        let snapshot_hash = hash(env, 1);
        let source_data_hash = hash(env, 11);
        let payload = signed_payload(env, 1, &snapshot_hash, &source_data_hash);
        let signature = sign(env, &signing_key, &payload);
        target.mock_all_auths().submit_snapshot(
            &1,
            &snapshot_hash,
            &source_data_hash,
            &signature,
            &admin,
        );
    }
    (manager_id, target_id, admin, signing_public_key)
}

#[test]
fn wrong_schema_is_rejected_until_authorized_migration() {
    let env = Env::default();
    let (manager_id, target_id, _admin, _key) = setup(&env, false);
    let manager = UpgradeManagerClient::new(&env, &manager_id);
    let v2_wasm = fixture_wasm("stellar_insights_v2");
    let v2_hash = env.deployer().upload_contract_wasm(v2_wasm.as_slice());

    let proposal_id = manager
        .mock_all_auths()
        .create_proposal(&target_id, &v2_hash, &1, &2);
    manager
        .mock_all_auths()
        .record_realistic_storage_test(&proposal_id, &hash(&env, 42));
    let config = manager.get_config();
    manager
        .mock_all_auths()
        .approve_upgrade(&proposal_id, &config.approvers.get(0).unwrap());
    manager.execute_upgrade(&proposal_id);

    let v2 = V2Client::new(&env, &target_id);
    expect_target_panic(
        v2.try_latest_epoch(),
        test_support::TargetError::SchemaMismatch,
    );
    expect_target_error(
        v2.try_get_snapshot(&1),
        test_support::TargetError::SchemaMismatch,
    );

    manager.migrate_upgrade(&proposal_id);
    assert_eq!(v2.latest_epoch(), 1);
    assert_eq!(v2.get_snapshot(&1).epoch, 1);

    // V2 cannot be used to relabel its storage to an arbitrary schema.
    expect_target_error(
        v2.mock_all_auths().try_migrate_schema(&2, &99),
        test_support::TargetError::InvalidSchemaTransition,
    );
    expect_target_error(
        v2.mock_all_auths().try_migrate_schema(&99, &2),
        test_support::TargetError::InvalidSchemaTransition,
    );
    assert_eq!(v2.latest_epoch(), 1);
}

#[test]
fn missing_schema_metadata_is_not_treated_as_current() {
    let env = Env::default();
    let (manager_id, target_id, _admin, _key) = setup(&env, true);
    let manager = UpgradeManagerClient::new(&env, &manager_id);
    let v2_wasm = fixture_wasm("stellar_insights_v2");
    let v2_hash = env.deployer().upload_contract_wasm(v2_wasm.as_slice());

    let proposal_id = manager
        .mock_all_auths()
        .create_proposal(&target_id, &v2_hash, &0, &2);
    manager
        .mock_all_auths()
        .record_realistic_storage_test(&proposal_id, &hash(&env, 43));
    let config = manager.get_config();
    manager
        .mock_all_auths()
        .approve_upgrade(&proposal_id, &config.approvers.get(0).unwrap());
    manager.execute_upgrade(&proposal_id);

    let v2 = V2Client::new(&env, &target_id);
    expect_target_panic(
        v2.try_latest_epoch(),
        test_support::TargetError::SchemaMismatch,
    );
    manager.migrate_upgrade(&proposal_id);
    assert_eq!(v2.latest_epoch(), 1);
}

#[test]
fn invalid_schema_transition_is_rejected_by_manager() {
    let env = Env::default();
    let (manager_id, target_id, _admin, _key) = setup(&env, false);
    let manager = UpgradeManagerClient::new(&env, &manager_id);
    let v2_hash = env
        .deployer()
        .upload_contract_wasm(fixture_wasm("stellar_insights_v2").as_slice());
    expect_manager_error(
        manager
            .mock_all_auths()
            .try_create_proposal(&target_id, &v2_hash, &1, &0),
        ManagerError::InvalidSchemaTransition,
    );

    let production = ProductionClient::new(&env, &target_id);
    expect_target_error(
        production.mock_all_auths().try_migrate_schema(&1, &99),
        test_support::TargetError::InvalidSchemaTransition,
    );
    expect_target_error(
        production.mock_all_auths().try_migrate_schema(&99, &1),
        test_support::TargetError::InvalidSchemaTransition,
    );
    assert_eq!(production.latest_epoch(), 1);
}
