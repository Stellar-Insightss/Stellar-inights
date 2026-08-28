#![cfg(feature = "testutils")]

mod test_support;

use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    Address, Env, IntoVal, Symbol,
};
use test_support::{expect_target_error, fixture_wasm, signing_key, ProductionClient, TargetError};

#[test]
fn upgrade_manager_bootstrap_cannot_be_replaced() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let (signing_public_key, _) = signing_key(&env);
    let first_manager = Address::generate(&env);
    let replacement_manager = Address::generate(&env);
    let target_id = env.register(fixture_wasm("stellar_insights").as_slice(), ());
    let target = ProductionClient::new(&env, &target_id);

    target
        .mock_all_auths()
        .initialize(&admin, &signing_public_key);
    target.mock_all_auths().set_upgrade_manager(&first_manager);
    assert_eq!(
        env.auths(),
        [(
            admin.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    target_id.clone(),
                    Symbol::new(&env, "set_upgrade_manager"),
                    (&first_manager,).into_val(&env),
                )),
                sub_invocations: [].into(),
            }
        )]
    );

    expect_target_error(
        target
            .mock_all_auths()
            .try_set_upgrade_manager(&replacement_manager),
        TargetError::UpgradeManagerAlreadySet,
    );
    expect_target_error(
        target
            .mock_all_auths()
            .try_set_upgrade_manager(&first_manager),
        TargetError::UpgradeManagerAlreadySet,
    );
}
